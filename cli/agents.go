package main

import (
	"fmt"
	"net/http"
	"os"
	"text/tabwriter"
)

type agentDetail struct {
	ID            string `json:"id"`
	Name          string `json:"name"`
	Status        string `json:"status"`
	Circle        string `json:"circle"`
	LifecycleMode string `json:"lifecycle_mode"`
}

func runAgents(args []string) {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "usage: sera agents <list|start|stop|restart|logs> [args]")
		os.Exit(1)
	}

	sub := args[0]
	rest := args[1:]

	client, err := NewClient()
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}

	switch sub {
	case "list":
		runAgentsList(client)
	case "start":
		runAgentAction(client, http.MethodPost, "start", rest)
	case "stop":
		runAgentAction(client, http.MethodPost, "stop", rest)
	case "restart":
		runAgentAction(client, http.MethodPost, "restart", rest)
	case "logs":
		runAgentLogs(client, rest)
	default:
		fmt.Fprintf(os.Stderr, "unknown agents subcommand: %s\n", sub)
		os.Exit(1)
	}
}

func runAgentsList(c *SeraClient) {
	var agents []agentDetail
	if err := c.Do(http.MethodGet, "/api/agents/instances", nil, &agents); err != nil {
		fmt.Fprintf(os.Stderr, "error listing agents: %v\n", err)
		os.Exit(1)
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "ID\tNAME\tSTATUS\tCIRCLE\tMODE")
	for _, a := range agents {
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", a.ID, a.Name, a.Status, a.Circle, a.LifecycleMode)
	}
	w.Flush()
}

func runAgentAction(c *SeraClient, method, action string, args []string) {
	if len(args) == 0 {
		fmt.Fprintf(os.Stderr, "usage: sera agents %s <id|name>\n", action)
		os.Exit(1)
	}

	id, err := c.ResolveAgentID(args[0])
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving agent: %v\n", err)
		os.Exit(1)
	}

	path := fmt.Sprintf("/api/agents/instances/%s/%s", id, action)
	if action == "restart" {
		path = fmt.Sprintf("/api/agents/%s/restart", id)
	}

	if err := c.Do(method, path, nil, nil); err != nil {
		fmt.Fprintf(os.Stderr, "error %s agent: %v\n", action, err)
		os.Exit(1)
	}

	fmt.Printf("Agent %s %sed successfully.\n", args[0], action)
}

func runAgentLogs(c *SeraClient, args []string) {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "usage: sera agents logs <id|name>")
		os.Exit(1)
	}

	id, err := c.ResolveAgentID(args[0])
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving agent: %v\n", err)
		os.Exit(1)
	}

	var logs string
	if err := c.Do(http.MethodGet, fmt.Sprintf("/api/agents/%s/logs", id), nil, &logs); err != nil {
		fmt.Fprintf(os.Stderr, "error fetching logs: %v\n", err)
		os.Exit(1)
	}

	fmt.Println(logs)
}
