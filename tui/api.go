package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

// APIClient interacts with the SERA Core backend.
type APIClient struct {
	BaseURL string
	client  *http.Client
}

// NewAPIClient creates a new SERA API client.
func NewAPIClient(baseURL string) *APIClient {
	return &APIClient{
		BaseURL: baseURL,
		client: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// GetInstances fetches available agent instances.
func (c *APIClient) GetInstances() ([]AgentInstance, error) {
	resp, err := c.client.Get(fmt.Sprintf("%s/api/agents/instances", c.BaseURL))
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("unexpected status: %s", resp.Status)
	}

	var instances []AgentInstance
	if err := json.NewDecoder(resp.Body).Decode(&instances); err != nil {
		return nil, err
	}
	return instances, nil
}

// SendChatStream triggers a streaming chat response.
func (c *APIClient) SendChatStream(req ChatRequest) (*ChatResponse, error) {
	req.Stream = true
	body, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}

	resp, err := c.client.Post(
		fmt.Sprintf("%s/api/chat", c.BaseURL),
		"application/json",
		bytes.NewBuffer(body),
	)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		var errData struct {
			Error string `json:"error"`
		}
		json.NewDecoder(resp.Body).Decode(&errData)
		if errData.Error != "" {
			return nil, fmt.Errorf("api error: %s", errData.Error)
		}
		return nil, fmt.Errorf("unexpected status: %s", resp.Status)
	}

	var chatResp ChatResponse
	if err := json.NewDecoder(resp.Body).Decode(&chatResp); err != nil {
		return nil, err
	}
	return &chatResp, nil
}
