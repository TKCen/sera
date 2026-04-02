package main

import (
	"testing"

	tea "github.com/charmbracelet/bubbletea"
)

func TestNewWSClient(t *testing.T) {
	url := "ws://localhost:8000/connection/websocket"
	ws := NewWSClient(url, nil)

	if ws == nil {
		t.Fatal("expected WSClient to be non-nil")
	}

	if ws.client == nil {
		t.Error("expected centrifuge client to be initialized")
	}

	if ws.program != nil {
		t.Error("expected program to be nil")
	}
}

func TestNewWSClientWithProgram(t *testing.T) {
	url := "ws://localhost:8000/connection/websocket"
	program := &tea.Program{}
	ws := NewWSClient(url, program)

	if ws == nil {
		t.Fatal("expected WSClient to be non-nil")
	}

	if ws.program != program {
		t.Errorf("expected program %v, got %v", program, ws.program)
	}
}
