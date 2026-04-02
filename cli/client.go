package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

type SeraClient struct {
	BaseURL string
	APIKey  string
}

func NewClient() (*SeraClient, error) {
	apiKey := Getenv("SERA_API_KEY", "")
	if apiKey == "" {
		creds, err := ReadCredentials()
		if err == nil {
			apiKey = creds.APIKey
			if creds.ExpiresAt != "" {
				expiry, parseErr := time.Parse(time.RFC3339, creds.ExpiresAt)
				if parseErr == nil && time.Now().After(expiry) {
					return nil, fmt.Errorf("token has expired. Run: sera auth login")
				}
			}
		}
	}

	if apiKey == "" {
		return nil, fmt.Errorf("not logged in. Run: sera auth login")
	}

	return &SeraClient{
		BaseURL: Getenv("SERA_API_URL", "http://localhost:3001"),
		APIKey:  apiKey,
	}, nil
}

func (c *SeraClient) Do(method, path string, body interface{}, result interface{}) error {
	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return err
		}
		bodyReader = bytes.NewReader(data)
	}

	req, err := http.NewRequest(method, c.BaseURL+path, bodyReader)
	if err != nil {
		return err
	}

	req.Header.Set("Authorization", "Bearer "+c.APIKey)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		respBody, _ := io.ReadAll(resp.Body)
		var errResp struct {
			Error string `json:"error"`
		}
		if err := json.Unmarshal(respBody, &errResp); err == nil && errResp.Error != "" {
			return fmt.Errorf("HTTP %d: %s", resp.StatusCode, errResp.Error)
		}
		return fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(respBody))
	}

	if result != nil {
		if r, ok := result.(*string); ok {
			data, err := io.ReadAll(resp.Body)
			if err != nil {
				return err
			}
			*r = string(data)
			return nil
		}
		return json.NewDecoder(resp.Body).Decode(result)
	}

	return nil
}

type agentInstance struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

func (c *SeraClient) ResolveAgentID(nameOrID string) (string, error) {
	// If it looks like a UUID, return it
	if strings.Contains(nameOrID, "-") && len(nameOrID) >= 32 {
		return nameOrID, nil
	}

	var instances []agentInstance
	if err := c.Do(http.MethodGet, "/api/agents/instances", nil, &instances); err != nil {
		return "", err
	}

	for _, inst := range instances {
		if inst.Name == nameOrID || inst.ID == nameOrID {
			return inst.ID, nil
		}
	}

	return "", fmt.Errorf("agent not found: %s", nameOrID)
}
