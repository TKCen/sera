package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"time"
)

const CredentialsFile = ".sera/credentials"

// Credentials written to ~/.sera/credentials (mode 0600).
type Credentials struct {
	APIKey    string `json:"apiKey"`
	Issuer    string `json:"issuer"`
	ExpiresAt string `json:"expiresAt"`
}

type SeraClient struct {
	BaseURL string
	APIKey  string
}

func NewSeraClient() (*SeraClient, error) {
	apiURL := Getenv("SERA_API_URL", "http://localhost:3001")
	apiKey := Getenv("SERA_API_KEY", "")

	if apiKey == "" {
		creds, err := ReadCredentials()
		if err != nil {
			return nil, fmt.Errorf("not logged in. Run: sera auth login")
		}
		apiKey = creds.APIKey
		if creds.ExpiresAt != "" {
			expiry, _ := time.Parse(time.RFC3339, creds.ExpiresAt)
			if time.Now().After(expiry) {
				return nil, fmt.Errorf("token has expired. Run: sera auth login")
			}
		}
	}

	return &SeraClient{BaseURL: apiURL, APIKey: apiKey}, nil
}

func (c *SeraClient) Do(method, path string, body interface{}) (*http.Response, error) {
	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		bodyReader = bytes.NewReader(data)
	}

	req, err := http.NewRequest(method, c.BaseURL+path, bodyReader)
	if err != nil {
		return nil, err
	}

	req.Header.Set("Authorization", "Bearer "+c.APIKey)
	req.Header.Set("Content-Type", "application/json")

	return http.DefaultClient.Do(req)
}

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

func WriteCredentials(creds Credentials) error {
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
