package main

import (
	"encoding/json"
	"fmt"
	"log"

	"github.com/centrifugal/centrifuge-go"
	tea "github.com/charmbracelet/bubbletea"
)

// WSClient manages the Centrifugo WebSocket connection.
type WSClient struct {
	client *centrifuge.Client
	program *tea.Program
}

// NewWSClient initializes the Centrifugo client.
func NewWSClient(url string, program *tea.Program) *WSClient {
	c := centrifuge.NewJsonClient(url, centrifuge.Config{})
	return &WSClient{
		client:  c,
		program: program,
	}
}

// Connect starts the WebSocket connection.
func (ws *WSClient) Connect() error {
	return ws.client.Connect()
}

// Disconnect closes the WebSocket connection.
func (ws *WSClient) Disconnect() error {
	return ws.client.Disconnect()
}

// SubscribeToThoughts listens for agent reasoning steps.
func (ws *WSClient) SubscribeToThoughts(agentID string) error {
	channel := fmt.Sprintf("internal:agent:%s:thoughts", agentID)
	sub, err := ws.client.NewSubscription(channel)
	if err != nil {
		return err
	}

	sub.OnPublication(func(e centrifuge.PublicationEvent) {
		var event ThoughtEvent
		if err := json.Unmarshal(e.Data, &event); err != nil {
			log.Printf("Failed to unmarshal thought event: %v", err)
			return
		}
		ws.program.Send(thoughtMsg(event))
	})

	return sub.Subscribe()
}

// SubscribeToStream listens for text tokens of a specific message.
func (ws *WSClient) SubscribeToStream(messageID string) error {
	channel := fmt.Sprintf("internal:stream:%s", messageID)
	sub, err := ws.client.NewSubscription(channel)
	if err != nil {
		return err
	}

	sub.OnPublication(func(e centrifuge.PublicationEvent) {
		var token StreamToken
		if err := json.Unmarshal(e.Data, &token); err != nil {
			log.Printf("Failed to unmarshal stream token: %v", err)
			return
		}
		if token.Token != "" {
			ws.program.Send(tokenMsg(token.Token))
		}
		if token.Done {
			ws.program.Send(streamDoneMsg{})
			sub.Unsubscribe()
		}
	})

	return sub.Subscribe()
}
