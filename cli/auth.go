package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const CredentialsFile = ".sera/credentials"

// Credentials written to ~/.sera/credentials (mode 0600).
// JSON format: { "apiKey": "...", "issuer": "...", "expiresAt": "..." }
type Credentials struct {
	APIKey    string `json:"apiKey"`
	Issuer    string `json:"issuer"`
	ExpiresAt string `json:"expiresAt"`
}

// deviceAuthResponse mirrors the RFC 8628 device authorization response.
type deviceAuthResponse struct {
	DeviceCode              string `json:"device_code"`
	UserCode                string `json:"user_code"`
	VerificationURI         string `json:"verification_uri"`
	VerificationURIComplete string `json:"verification_uri_complete"`
	ExpiresIn               int    `json:"expires_in"`
	Interval                int    `json:"interval"`
}

// tokenResponse mirrors a successful RFC 8628 token response.
type tokenResponse struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	ExpiresIn    int    `json:"expires_in"`
	Error        string `json:"error"`
}

// meResponse mirrors GET /api/auth/me from sera-core.
type meResponse struct {
	Sub   string   `json:"sub"`
	Name  string   `json:"name"`
	Email string   `json:"email"`
	Roles []string `json:"roles"`
}

func runAuth(args []string) {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "usage: sera auth <login|logout|status>")
		os.Exit(1)
	}

	sub := args[0]
	rest := args[1:]

	switch sub {
	case "login":
		runAuthLogin(rest)
	case "logout":
		runAuthLogout()
	case "status":
		runAuthStatus()
	default:
		fmt.Fprintf(os.Stderr, "unknown auth subcommand: %s\n", sub)
		os.Exit(1)
	}
}

// ── sera auth login ──────────────────────────────────────────────────────────

func runAuthLogin(args []string) {
	serviceAccount := len(args) > 0 && args[0] == "--service-account"

	apiURL := Getenv("SERA_API_URL", "http://localhost:3001")

	// Resolve OIDC metadata from sera-core
	issuerURL, clientID, err := resolveOIDCConfig(apiURL)
	if err != nil {
		fmt.Fprintf(os.Stderr, "could not resolve OIDC config: %v\n", err)
		os.Exit(1)
	}

	deviceAuthURL := resolveDeviceAuthEndpoint(issuerURL)
	tokenURL := resolveTokenEndpoint(issuerURL)

	// Step 1 — request device code
	dar, err := requestDeviceCode(deviceAuthURL, clientID)
	if err != nil {
		fmt.Fprintf(os.Stderr, "device authorization failed: %v\n", err)
		os.Exit(1)
	}

	// Step 2 — display instructions
	fmt.Printf("\nTo sign in, visit:\n\n  %s\n\n", dar.VerificationURI)
	fmt.Printf("And enter code: %s\n\n", dar.UserCode)
	if dar.VerificationURIComplete != "" {
		fmt.Printf("Or open: %s\n\n", dar.VerificationURIComplete)
	}
	fmt.Println("Waiting for authentication…")

	// Step 3 — poll for token
	interval := dar.Interval
	if interval == 0 {
		interval = 5
	}
	deadline := time.Now().Add(time.Duration(dar.ExpiresIn) * time.Second)

	var tokens *tokenResponse
	for time.Now().Before(deadline) {
		time.Sleep(time.Duration(interval) * time.Second)
		tokens, err = pollToken(tokenURL, clientID, dar.DeviceCode)
		if err != nil {
			fmt.Fprintf(os.Stderr, "token poll error: %v\n", err)
			os.Exit(1)
		}
		if tokens != nil {
			break
		}
	}

	if tokens == nil {
		fmt.Fprintln(os.Stderr, "authentication timed out")
		os.Exit(1)
	}

	expiresAt := time.Now().Add(time.Duration(tokens.ExpiresIn) * time.Second).UTC().Format(time.RFC3339)

	if serviceAccount {
		// --service-account: create a long-lived API key via sera-core
		apiKey, err := createServiceAccountKey(apiURL, tokens.AccessToken)
		if err != nil {
			fmt.Fprintf(os.Stderr, "failed to create service account key: %v\n", err)
			os.Exit(1)
		}
		creds := Credentials{
			APIKey:    apiKey,
			Issuer:    issuerURL,
			ExpiresAt: "", // API keys don't expire by default
		}
		if writeErr := writeCredentials(creds); writeErr != nil {
			fmt.Fprintf(os.Stderr, "warning: could not write credentials: %v\n", writeErr)
		}
		fmt.Printf("\nService account key created and stored in ~/%s\n", CredentialsFile)
		fmt.Println("Use --api-key or set SERA_API_KEY for non-interactive use.")
		return
	}

	// Store access token as the credential (used as Bearer token against sera-core)
	creds := Credentials{
		APIKey:    tokens.AccessToken,
		Issuer:    issuerURL,
		ExpiresAt: expiresAt,
	}
	if writeErr := writeCredentials(creds); writeErr != nil {
		fmt.Fprintf(os.Stderr, "warning: could not write credentials: %v\n", writeErr)
	}

	fmt.Printf("\nAuthenticated successfully. Credentials stored in ~/%s\n", CredentialsFile)
	if tokens.ExpiresIn > 0 {
		fmt.Printf("Token expires: %s\n", expiresAt)
	}
}

// ── sera auth logout ─────────────────────────────────────────────────────────

func runAuthLogout() {
	creds, err := ReadCredentials()
	if err == nil && creds.APIKey != "" {
		// Best-effort token revocation
		apiURL := Getenv("SERA_API_URL", "http://localhost:3001")
		revokeToken(apiURL, creds)
	}

	path := CredentialsPath()
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		fmt.Fprintf(os.Stderr, "warning: could not remove credentials file: %v\n", err)
	}
	fmt.Println("Logged out. Credentials removed.")
}

// ── sera auth status ─────────────────────────────────────────────────────────

func runAuthStatus() {
	// Check for --api-key override
	apiKey := Getenv("SERA_API_KEY", "")
	if apiKey == "" {
		creds, err := ReadCredentials()
		if err != nil {
			fmt.Println("Not logged in. Run: sera auth login")
			return
		}
		apiKey = creds.APIKey
		if creds.ExpiresAt != "" {
			expiry, parseErr := time.Parse(time.RFC3339, creds.ExpiresAt)
			if parseErr == nil && time.Now().After(expiry) {
				fmt.Println("Token has expired. Run: sera auth login")
				return
			}
		}
	}

	apiURL := Getenv("SERA_API_URL", "http://localhost:3001")
	me, err := fetchMe(apiURL, apiKey)
	if err != nil {
		fmt.Fprintf(os.Stderr, "could not fetch identity: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Logged in as: %s", me.Sub)
	if me.Name != "" {
		fmt.Printf(" (%s)", me.Name)
	}
	if me.Email != "" {
		fmt.Printf(" <%s>", me.Email)
	}
	fmt.Println()
	if len(me.Roles) > 0 {
		fmt.Printf("Roles: %s\n", strings.Join(me.Roles, ", "))
	}
}

// ── Helpers ──────────────────────────────────────────────────────────────────

func Getenv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func CredentialsPath() string {
	home, err := os.UserHomeDir()
	if err != nil {
		home = "."
	}
	return filepath.Join(home, CredentialsFile)
}

func writeCredentials(creds Credentials) error {
	path := CredentialsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	data, err := json.MarshalIndent(creds, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, data, 0600)
}

func ReadCredentials() (Credentials, error) {
	path := CredentialsPath()
	data, err := os.ReadFile(path)
	if err != nil {
		return Credentials{}, err
	}
	var creds Credentials
	return creds, json.Unmarshal(data, &creds)
}

func resolveOIDCConfig(apiURL string) (issuerURL, clientID string, err error) {
	resp, err := http.Get(apiURL + "/api/auth/oidc-config")
	if err != nil {
		// sera-core might not have this endpoint — fall back to env
		issuerURL = Getenv("OIDC_ISSUER_URL", "")
		clientID = Getenv("OIDC_CLIENT_ID", "sera-web")
		if issuerURL == "" {
			return "", "", fmt.Errorf("OIDC_ISSUER_URL not set and /api/auth/oidc-config unavailable")
		}
		return issuerURL, clientID, nil
	}
	defer resp.Body.Close()

	var cfg struct {
		IssuerURL string `json:"issuerUrl"`
		ClientID  string `json:"clientId"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&cfg); err != nil || cfg.IssuerURL == "" {
		issuerURL = Getenv("OIDC_ISSUER_URL", "")
		clientID = Getenv("OIDC_CLIENT_ID", "sera-web")
		if issuerURL == "" {
			return "", "", fmt.Errorf("could not resolve OIDC issuer URL")
		}
		return issuerURL, clientID, nil
	}
	return cfg.IssuerURL, cfg.ClientID, nil
}

func resolveDeviceAuthEndpoint(issuerURL string) string {
	base := strings.TrimRight(issuerURL, "/")
	if strings.Contains(base, "/realms/") || strings.Contains(base, "/application/o/") {
		return base + "/protocol/openid-connect/auth/device"
	}
	return base + "/device_authorization"
}

func resolveTokenEndpoint(issuerURL string) string {
	base := strings.TrimRight(issuerURL, "/")
	if strings.Contains(base, "/realms/") || strings.Contains(base, "/application/o/") {
		return base + "/protocol/openid-connect/token"
	}
	return base + "/token"
}

func requestDeviceCode(deviceAuthURL, clientID string) (*deviceAuthResponse, error) {
	resp, err := http.PostForm(deviceAuthURL, url.Values{
		"client_id": {clientID},
		"scope":     {"openid profile email"},
	})
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(body))
	}

	var dar deviceAuthResponse
	if err := json.Unmarshal(body, &dar); err != nil {
		return nil, err
	}
	return &dar, nil
}

func pollToken(tokenURL, clientID, deviceCode string) (*tokenResponse, error) {
	resp, err := http.PostForm(tokenURL, url.Values{
		"grant_type":  {"urn:ietf:params:oauth:grant-type:device_code"},
		"client_id":   {clientID},
		"device_code": {deviceCode},
	})
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var tr tokenResponse
	if err := json.NewDecoder(resp.Body).Decode(&tr); err != nil {
		return nil, err
	}

	switch tr.Error {
	case "":
		return &tr, nil
	case "authorization_pending", "slow_down":
		return nil, nil // keep polling
	case "expired_token":
		return nil, fmt.Errorf("device code expired")
	default:
		return nil, fmt.Errorf("token error: %s", tr.Error)
	}
}

func fetchMe(apiURL, apiKey string) (*meResponse, error) {
	req, err := http.NewRequest(http.MethodGet, apiURL+"/api/auth/me", nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer "+apiKey)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("HTTP %d from /api/auth/me", resp.StatusCode)
	}

	var me meResponse
	return &me, json.NewDecoder(resp.Body).Decode(&me)
}

func createServiceAccountKey(apiURL, accessToken string) (string, error) {
	reqBody := strings.NewReader(`{"name":"sera-cli-service-account","roles":["operator"]}`)
	req, err := http.NewRequest(http.MethodPost, apiURL+"/api/auth/api-keys", reqBody)
	if err != nil {
		return "", err
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusCreated {
		return "", fmt.Errorf("HTTP %d creating API key", resp.StatusCode)
	}

	var result struct {
		Key string `json:"key"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", err
	}
	return result.Key, nil
}

func revokeToken(apiURL string, creds Credentials) {
	req, err := http.NewRequest(http.MethodPost, apiURL+"/api/auth/logout", nil)
	if err != nil {
		return
	}
	req.Header.Set("Authorization", "Bearer "+creds.APIKey)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return
	}
	resp.Body.Close()
}
