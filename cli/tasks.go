package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"text/tabwriter"
	"time"
)

type taskDetail struct {
	ID        string    `json:"id"`
	Task      string    `json:"task"`
	Status    string    `json:"status"`
	CreatedAt time.Time `json:"createdAt"`
}

func runTasks(args []string) {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "usage: sera tasks <list|create|get|cancel> <agent-id|name> [args]")
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
		runTasksList(client, rest)
	case "create":
		runTasksCreate(client, rest)
	case "get":
		runTasksGet(client, rest)
	case "cancel":
		runTasksCancel(client, rest)
	default:
		fmt.Fprintf(os.Stderr, "unknown tasks subcommand: %s\n", sub)
		os.Exit(1)
	}
}

func runTasksList(c *SeraClient, args []string) {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "usage: sera tasks list <agent-id|name>")
		os.Exit(1)
	}

	agentID, err := c.ResolveAgentID(args[0])
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving agent: %v\n", err)
		os.Exit(1)
	}

	var tasks []taskDetail
	if err := c.Do(http.MethodGet, fmt.Sprintf("/api/agents/%s/tasks", agentID), nil, &tasks); err != nil {
		fmt.Fprintf(os.Stderr, "error listing tasks: %v\n", err)
		os.Exit(1)
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "ID\tSTATUS\tCREATED\tTASK")
	for _, t := range tasks {
		summary := t.Task
		if len(summary) > 50 {
			summary = summary[:47] + "..."
		}
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", t.ID, t.Status, t.CreatedAt.Format("2006-01-02 15:04"), summary)
	}
	w.Flush()
}

func runTasksCreate(c *SeraClient, args []string) {
	if len(args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: sera tasks create <agent-id|name> <prompt>")
		os.Exit(1)
	}

	agentID, err := c.ResolveAgentID(args[0])
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving agent: %v\n", err)
		os.Exit(1)
	}

	prompt := args[1]
	body := map[string]string{"task": prompt}
	var resp struct {
		TaskID string `json:"taskId"`
		Status string `json:"status"`
	}

	if err := c.Do(http.MethodPost, fmt.Sprintf("/api/agents/%s/tasks", agentID), body, &resp); err != nil {
		fmt.Fprintf(os.Stderr, "error creating task: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Task created: %s (status: %s)\n", resp.TaskID, resp.Status)
}

func runTasksGet(c *SeraClient, args []string) {
	if len(args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: sera tasks get <agent-id|name> <task-id>")
		os.Exit(1)
	}

	agentID, err := c.ResolveAgentID(args[0])
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving agent: %v\n", err)
		os.Exit(1)
	}

	taskID := args[1]
	var task interface{}
	if err := c.Do(http.MethodGet, fmt.Sprintf("/api/agents/%s/tasks/%s", agentID, taskID), nil, &task); err != nil {
		fmt.Fprintf(os.Stderr, "error fetching task: %v\n", err)
		os.Exit(1)
	}

	pretty, _ := json.MarshalIndent(task, "", "  ")
	fmt.Println(string(pretty))
}

func runTasksCancel(c *SeraClient, args []string) {
	if len(args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: sera tasks cancel <agent-id|name> <task-id>")
		os.Exit(1)
	}

	agentID, err := c.ResolveAgentID(args[0])
	if err != nil {
		fmt.Fprintf(os.Stderr, "error resolving agent: %v\n", err)
		os.Exit(1)
	}

	taskID := args[1]
	if err := c.Do(http.MethodDelete, fmt.Sprintf("/api/agents/%s/tasks/%s", agentID, taskID), nil, nil); err != nil {
		fmt.Fprintf(os.Stderr, "error cancelling task: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Task %s cancelled successfully.\n", taskID)
}
