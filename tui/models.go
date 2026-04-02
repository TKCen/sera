package main

import "time"

// AgentInstance represents a SERA agent instance.
type AgentInstance struct {
	ID           string `json:"id"`
	Name         string `json:"name"`
	TemplateName string `json:"templateName"`
	Status       string `json:"status"`
}

// ChatRequest is the payload for the /api/chat endpoint.
type ChatRequest struct {
	Message         string `json:"message"`
	SessionID       string `json:"sessionId,omitempty"`
	AgentInstanceID string `json:"agentInstanceId,omitempty"`
	Stream          bool   `json:"stream"`
}

// ChatResponse is the response from the /api/chat endpoint.
type ChatResponse struct {
	SessionID string `json:"sessionId"`
	MessageID string `json:"messageId"`
}

// ThoughtEvent represents a reasoning step published via Centrifugo.
type ThoughtEvent struct {
	Timestamp        string `json:"timestamp"`
	StepType         string `json:"stepType"`
	Content          string `json:"content"`
	AgentID          string `json:"agentId"`
	AgentDisplayName string `json:"agentDisplayName"`
}

// StreamToken represents a text chunk published via Centrifugo.
type StreamToken struct {
	Token     string `json:"token"`
	Done      bool   `json:"done"`
	MessageID string `json:"messageId"`
}

// ChatMessage represents a single message in the TUI history.
type ChatMessage struct {
	Sender    string
	Text      string
	Timestamp time.Time
}

// Internal Bubble Tea messages
type errMsg error
type instancesMsg []AgentInstance
type chatResponseMsg ChatResponse
type tokenMsg string
type thoughtMsg ThoughtEvent
type streamDoneMsg struct{}
