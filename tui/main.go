package main

import (
	"fmt"
	"log"
	"os"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/list"
	"github.com/charmbracelet/bubbles/textarea"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"
)

var (
	baseURL = getEnv("SERA_API_URL", "http://localhost:3001")
	wsURL   = getEnv("SERA_WS_URL", "ws://localhost:10001/connection/websocket")
)

func getEnv(key, fallback string) string {
	if value, ok := os.LookupEnv(key); ok {
		return value
	}
	return fallback
}

type state int

const (
	stateSelecting state = iota
	stateChatting
)

type model struct {
	state           state
	api             *APIClient
	ws              *WSClient
	list            list.Model
	viewport        viewport.Model
	textarea        textarea.Model
	thoughtsView    viewport.Model
	selectedAgent   *AgentInstance
	messages        []ChatMessage
	thoughts        []ThoughtEvent
	currentResponse string
	sessionId       string
	isLoading       bool
	width           int
	height          int
	err             error
	renderer        *glamour.TermRenderer
}

type agentItem AgentInstance

func (i agentItem) Title() string       { return i.Name }
func (i agentItem) Description() string { return fmt.Sprintf("%s (%s)", i.TemplateName, i.Status) }
func (i agentItem) FilterValue() string { return i.Name }

func initialModel(api *APIClient, ws *WSClient) model {
	ta := textarea.New()
	ta.Placeholder = "Send a message..."
	ta.Focus()
	ta.Prompt = "┃ "
	ta.CharLimit = 1000
	ta.SetWidth(50)
	ta.SetHeight(3)
	ta.FocusedStyle.CursorLine = lipgloss.NewStyle()
	ta.ShowLineNumbers = false
	ta.KeyMap.InsertNewline.SetEnabled(false)

	vp := viewport.New(50, 20)
	vp.SetContent("Welcome to SERA TUI! Select an agent to start.")

	tvp := viewport.New(30, 20)
	tvp.SetContent("Agent thoughts will appear here.")

	renderer, _ := glamour.NewTermRenderer(
		glamour.WithAutoStyle(),
		glamour.WithWordWrap(50),
	)

	return model{
		state:        stateSelecting,
		api:          api,
		ws:           ws,
		textarea:     ta,
		viewport:     vp,
		thoughtsView: tvp,
		messages:     []ChatMessage{},
		thoughts:     []ThoughtEvent{},
		renderer:     renderer,
	}
}

func (m model) Init() tea.Cmd {
	return m.fetchInstances
}

func (m model) fetchInstances() tea.Msg {
	instances, err := m.api.GetInstances()
	if err != nil {
		return errMsg(err)
	}
	return instancesMsg(instances)
}

func (m model) updateHistory() string {
	var b strings.Builder
	for _, msg := range m.messages {
		sender := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("5")).Render(msg.Sender + ": ")
		content, _ := m.renderer.Render(msg.Text)
		b.WriteString(sender + "\n" + content + "\n")
	}

	if m.currentResponse != "" {
		sender := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("2")).Render("SERA: ")
		content, _ := m.renderer.Render(m.currentResponse)
		b.WriteString(sender + "\n" + content + " ▮\n")
	} else if m.isLoading {
		b.WriteString(lipgloss.NewStyle().Italic(true).Foreground(lipgloss.Color("8")).Render("SERA is thinking...\n"))
	}

	return b.String()
}

func (m model) updateThoughts() string {
	var b strings.Builder
	for _, t := range m.thoughts {
		icon := "• "
		color := lipgloss.Color("8")
		switch t.StepType {
		case "observe":
			color = lipgloss.Color("4")
		case "plan":
			color = lipgloss.Color("3")
		case "act":
			color = lipgloss.Color("2")
		case "reflect":
			color = lipgloss.Color("13")
		case "tool-call":
			color = lipgloss.Color("6")
			icon = "🔧 "
		case "tool-result":
			color = lipgloss.Color("14")
			icon = "✅ "
		}
		style := lipgloss.NewStyle().Foreground(color)
		b.WriteString(style.Render(icon+strings.ToUpper(t.StepType)) + "\n")
		b.WriteString(lipgloss.NewStyle().Foreground(lipgloss.Color("240")).Render(t.Content) + "\n\n")
	}
	return b.String()
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd
	var cmds []tea.Cmd

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.viewport.Width = (m.width * 2 / 3) - 4
		m.viewport.Height = m.height - 10
		m.thoughtsView.Width = (m.width / 3) - 4
		m.thoughtsView.Height = m.height - 4
		m.textarea.SetWidth(m.viewport.Width)

		// Re-initialize renderer with new width
		m.renderer, _ = glamour.NewTermRenderer(
			glamour.WithAutoStyle(),
			glamour.WithWordWrap(m.viewport.Width),
		)

	case instancesMsg:
		items := make([]list.Item, len(msg))
		for i, inst := range msg {
			items[i] = agentItem(inst)
		}
		m.list = list.New(items, list.NewDefaultDelegate(), 0, 0)
		m.list.Title = "Select an Agent Instance"
		m.list.SetSize(m.width, m.height)

	case tea.KeyMsg:
		if m.state == stateSelecting {
			switch msg.String() {
			case "ctrl+c", "q":
				return m, tea.Quit
			case "enter":
				if i, ok := m.list.SelectedItem().(agentItem); ok {
					inst := AgentInstance(i)
					m.selectedAgent = &inst
					m.state = stateChatting
					m.viewport.SetContent(fmt.Sprintf("Connected to %s. Type a message to start.", inst.Name))
				}
			}
			m.list, cmd = m.list.Update(msg)
			return m, cmd
		}

		if m.state == stateChatting {
			switch msg.String() {
			case "ctrl+c", "esc":
				m.state = stateSelecting
				m.thoughts = []ThoughtEvent{}
				m.messages = []ChatMessage{}
				m.currentResponse = ""
				return m, nil
			case "enter":
				if m.textarea.Value() != "" && !m.isLoading {
					text := m.textarea.Value()
					m.messages = append(m.messages, ChatMessage{
						Sender:    "User",
						Text:      text,
						Timestamp: time.Now(),
					})
					m.textarea.Reset()
					m.isLoading = true
					m.currentResponse = ""
					m.viewport.SetContent(m.updateHistory())
					m.viewport.GotoBottom()

					// Send to API
					return m, func() tea.Msg {
						resp, err := m.api.SendChatStream(ChatRequest{
							Message:         text,
							SessionID:       m.sessionId,
							AgentInstanceID: m.selectedAgent.ID,
						})
						if err != nil {
							return errMsg(err)
						}
						return chatResponseMsg(*resp)
					}
				}
			}
		}

	case chatResponseMsg:
		m.sessionId = msg.SessionID
		// Subscribe to streams
		if m.ws != nil {
			m.ws.SubscribeToThoughts(m.selectedAgent.ID)
			m.ws.SubscribeToStream(msg.MessageID)
		}

	case tokenMsg:
		m.currentResponse += string(msg)
		m.viewport.SetContent(m.updateHistory())
		m.viewport.GotoBottom()

	case thoughtMsg:
		m.thoughts = append(m.thoughts, ThoughtEvent(msg))
		m.thoughtsView.SetContent(m.updateThoughts())
		m.thoughtsView.GotoBottom()

	case streamDoneMsg:
		m.messages = append(m.messages, ChatMessage{
			Sender:    "SERA",
			Text:      m.currentResponse,
			Timestamp: time.Now(),
		})
		m.currentResponse = ""
		m.isLoading = false
		m.viewport.SetContent(m.updateHistory())
		m.viewport.GotoBottom()

	case errMsg:
		m.err = msg
		log.Printf("Error: %v", msg)
	}

	if m.state == stateSelecting {
		m.list, cmd = m.list.Update(msg)
		cmds = append(cmds, cmd)
	} else {
		m.viewport, cmd = m.viewport.Update(msg)
		cmds = append(cmds, cmd)
		m.textarea, cmd = m.textarea.Update(msg)
		cmds = append(cmds, cmd)
		m.thoughtsView, cmd = m.thoughtsView.Update(msg)
		cmds = append(cmds, cmd)
	}

	return m, tea.Batch(cmds...)
}

func (m model) View() string {
	if m.err != nil {
		return lipgloss.NewStyle().Foreground(lipgloss.Color("9")).Render(fmt.Sprintf("Error: %v\nPress q to quit.", m.err))
	}

	if m.state == stateSelecting {
		return m.list.View()
	}

	header := lipgloss.NewStyle().
		Bold(true).
		Foreground(lipgloss.Color("2")).
		Padding(0, 1).
		Render(fmt.Sprintf("SERA Chat - %s", m.selectedAgent.Name))

	chatView := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color("240")).
		Padding(0, 1).
		Width(m.viewport.Width + 2).
		Render(m.viewport.View())

	inputView := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color("240")).
		Padding(0, 1).
		Width(m.viewport.Width + 2).
		Render(m.textarea.View())

	thoughtsHeader := lipgloss.NewStyle().
		Bold(true).
		Foreground(lipgloss.Color("4")).
		Padding(0, 1).
		Render("THOUGHTS")

	thoughtsView := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color("240")).
		Padding(0, 1).
		Width(m.thoughtsView.Width + 2).
		Height(m.height - 4).
		Render(m.thoughtsView.View())

	leftColumn := lipgloss.JoinVertical(lipgloss.Left, header, chatView, inputView)
	rightColumn := lipgloss.JoinVertical(lipgloss.Left, thoughtsHeader, thoughtsView)

	return lipgloss.JoinHorizontal(lipgloss.Top, leftColumn, rightColumn)
}

func main() {
	f, err := tea.LogToFile("debug.log", "debug")
	if err != nil {
		fmt.Println("fatal:", err)
		os.Exit(1)
	}
	defer f.Close()

	api := NewAPIClient(baseURL)
	ws := NewWSClient(wsURL, nil)

	m := initialModel(api, ws)
	p := tea.NewProgram(m, tea.WithAltScreen())
	ws.program = p

	if err := ws.Connect(); err != nil {
		log.Printf("Failed to connect to Centrifugo: %v", err)
	}
	defer ws.Disconnect()

	if _, err := p.Run(); err != nil {
		fmt.Printf("Alas, there's been an error: %v", err)
		os.Exit(1)
	}
}
