// sera — CLI for the SERA agentic platform.
// Story 16.6: OIDC device authorization flow + credential management.
package main

import (
	"fmt"
	"os"
)

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	cmd := os.Args[1]
	switch cmd {
	case "auth":
		runAuth(os.Args[2:])
	case "help", "--help", "-h":
		printUsage()
	default:
		fmt.Fprintf(os.Stderr, "unknown command: %s\n", cmd)
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Print(`sera — SERA platform CLI

Usage:
  sera auth login              Authenticate via OIDC device flow
  sera auth login --api-key    Create a long-lived API key for scripting
  sera auth logout             Revoke session and delete stored credentials
  sera auth status             Show current identity and token expiry

Global flags:
  --api-key <key>   Bypass stored credentials; use this API key instead
  SERA_API_KEY      Environment variable alternative to --api-key
  SERA_API_URL      API base URL (default: http://localhost:3001)
`)
}
