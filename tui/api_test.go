package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestNewAPIClient(t *testing.T) {
	baseURL := "http://example.com"
	client := NewAPIClient(baseURL)

	if client.BaseURL != baseURL {
		t.Errorf("expected BaseURL %s, got %s", baseURL, client.BaseURL)
	}

	if client.client == nil {
		t.Fatal("expected http.Client to be initialized, got nil")
	}

	expectedTimeout := 10 * time.Second
	if client.client.Timeout != expectedTimeout {
		t.Errorf("expected timeout %v, got %v", expectedTimeout, client.client.Timeout)
	}
}

func TestGetInstances(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/agents/instances" {
			t.Errorf("expected path /api/agents/instances, got %s", r.URL.Path)
		}
		instances := []AgentInstance{
			{ID: "1", Name: "Test Agent", TemplateName: "test-template", Status: "running"},
		}
		json.NewEncoder(w).Encode(instances)
	}))
	defer server.Close()

	client := NewAPIClient(server.URL)
	instances, err := client.GetInstances()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if len(instances) != 1 {
		t.Errorf("expected 1 instance, got %d", len(instances))
	}
	if instances[0].Name != "Test Agent" {
		t.Errorf("expected Name 'Test Agent', got '%s'", instances[0].Name)
	}
}

func TestSendChatStream(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("expected POST method, got %s", r.Method)
		}
		var req ChatRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			t.Errorf("failed to decode request: %v", err)
		}
		if req.Message != "hello" {
			t.Errorf("expected message 'hello', got '%s'", req.Message)
		}

		resp := ChatResponse{
			SessionID: "session-123",
			MessageID: "msg-456",
		}
		json.NewEncoder(w).Encode(resp)
	}))
	defer server.Close()

	client := NewAPIClient(server.URL)
	resp, err := client.SendChatStream(ChatRequest{Message: "hello"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if resp.SessionID != "session-123" {
		t.Errorf("expected SessionID 'session-123', got '%s'", resp.SessionID)
	}
	if resp.MessageID != "msg-456" {
		t.Errorf("expected MessageID 'msg-456', got '%s'", resp.MessageID)
	}
}
