# SERA TUI

Go-based terminal UI for interacting with SERA agents. Standalone binary — not part of the npm workspace.

## Language and tooling

- **Language:** Go
- **Build:** `go build -o tui.exe .` (run from the `tui/` directory using an absolute path)
- **Test:** `go test ./...`
- **Dependencies:** managed by `go.mod` / `go.sum` — run `go mod tidy` after adding imports

```bash
# Build from workspace root (absolute path avoids cd persistence issue)
cd D:/projects/homelab/sera/tui && go build -o tui.exe .

# Run tests
cd D:/projects/homelab/sera/tui && go test ./...
```

## Structure

| File | Purpose |
|---|---|
| `main.go` | Entry point, TUI initialisation |
| `api.go` | sera-core REST client |
| `ws.go` | Centrifugo WebSocket subscription |
| `models.go` | Shared data types |
| `api_test.go` | API client tests |

## Configuration

- **API base URL**: `SERA_API_URL` environment variable (default: `http://localhost:3001`)
- **Auth**: uses the same API key mechanism as any other sera-core client — see `docs/epics/16-authentication-and-secrets.md` Story 16.3

## API reference

The TUI communicates with sera-core via REST. See `docs/openapi.yaml` for the full API surface and `docs/ARCHITECTURE.md` → Real-Time Messaging for Centrifugo channel names and message shapes.

## Learnings

_(Add TUI-specific discoveries here.)_
