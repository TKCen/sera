# sera-tui — Terminal UI for SERA

A modern terminal user interface for SERA built with Rust using [ratatui](https://ratatui.rs/) and [crossterm](https://github.com/crossterm-rs/crossterm).

Replaces the Go TUI (`tui/` directory) with a Rust implementation that integrates with the sera-core API.

## Features

- **Agent Dashboard**: View all agent instances in a sortable table
- **Agent Details**: Inspect individual agent configuration and status
- **Log Viewer**: Stream and scroll through agent logs
- **Real-time Updates**: Refresh data with `r` key
- **Keyboard Navigation**: Use arrow keys or vim bindings (j/k) to navigate

## Architecture

### Module Structure

- **main.rs**: Entry point, terminal lifecycle management, event loop
- **app.rs**: Application state, view management, event routing
- **api.rs**: HTTP client for sera-core REST API with authentication
- **views/agents.rs**: Table view for agent list with selection
- **views/agent_detail.rs**: Formatted display of single agent details
- **views/logs.rs**: Scrollable log viewer with pagination
- **ui.rs**: Shared UI styling helpers
- **views/mod.rs**: View trait for composable components

### Data Flow

```
main.rs (event loop)
  ↓
App (state + routing)
  ↓
Views (rendering) + ApiClient (data fetching)
```

## Building

From the workspace root:

```bash
cargo build -p sera-tui --release
```

The binary will be at `rust/target/release/sera-tui`.

## Running

```bash
# With defaults (http://localhost:3001)
./sera-tui

# With custom API URL
SERA_API_URL=http://api.example.com:3001 ./sera-tui

# With custom API key
SERA_API_KEY=your_key_here ./sera-tui
```

## Keyboard Controls

### Agent List View

- `j` / `↓` — Next agent
- `k` / `↑` — Previous agent
- `Enter` — View agent details
- `l` — View agent logs
- `r` — Refresh data
- `q` — Quit

### Detail & Log Views

- `Esc` / `Backspace` — Back to agent list
- `j` / `↓` — Scroll down (logs only)
- `k` / `↑` — Scroll up (logs only)

## Configuration

### Environment Variables

- `SERA_API_URL`: API base URL (default: `http://localhost:3001`)
- `SERA_API_KEY`: API authentication key (default: `sera_bootstrap_dev_123`)

## API Integration

The TUI communicates with sera-core via these endpoints:

- `GET /api/agents/instances` — List all agent instances
- `GET /api/agents/{id}` — Get agent details
- `GET /api/agents/{id}/logs` — Get agent logs

All requests include the `Authorization: Bearer {SERA_API_KEY}` header.

## Design Notes

- **Non-blocking UI**: Uses 250ms polling interval for responsive feel
- **Error Handling**: All API errors displayed in status bar
- **Minimal Dependencies**: Only ratatui 0.29, crossterm 0.28, and reqwest
- **Type Safety**: Leverages sera-domain types for API responses
- **Async Runtime**: Built on tokio for non-blocking I/O

## Future Enhancements

- [ ] Chat interface with streaming responses
- [ ] Real-time updates via WebSocket (Centrifugo)
- [ ] Agent creation/configuration UI
- [ ] Performance metrics dashboard
- [ ] Color-coded status indicators
- [ ] Search/filter agents by name or status
