# SERA TUI

Rust-based terminal UI for interacting with SERA agents. Standalone binary — not part of the npm workspace.

## Language and tooling

- **Language:** Rust (edition 2021)
- **Build:** `cargo build --release` (run from the `tui/` directory)
- **Run:** `cargo run` or `./target/release/sera-tui`
- **Test:** `cargo test`
- **Dependencies:** managed by `Cargo.toml` / `Cargo.lock` — run `cargo update` to refresh

## Structure

| File | Purpose |
|---|---|
| `src/main.rs` | Entry point, CLI args (clap), TUI event loop, App state |
| `src/api.rs` | sera-core REST client (ureq) |
| `src/ws.rs` | Centrifugo WebSocket client (tungstenite, runs in thread) |
| `src/models.rs` | Shared data types |
| `src/ui.rs` | ratatui rendering |

## Configuration (env vars)

| Variable | Default | Description |
|---|---|---|
| `SERA_API_URL` | `http://localhost:3001` | sera-core base URL |
| `SERA_WS_URL` | `ws://localhost:10001/connection/websocket` | Centrifugo WebSocket URL |
| `SERA_API_KEY` | `sera_bootstrap_dev_123` | API bearer token |

## Usage

### Interactive TUI

```bash
cargo run
# or
./target/release/sera-tui
```

- Arrow keys / j/k to navigate agent list
- Enter to select an agent and open chat
- Type message + Enter to send
- PageUp / PageDown to scroll chat history
- Esc to return to agent list
- q or Ctrl+C to quit

### Non-interactive CLI (`-p`)

Send a single prompt and print the response — useful for scripting:

```bash
# Send a prompt to the first available agent
sera-tui -p "What is the weather in London?"

# Target a specific agent by name or ID
sera-tui -p "Summarise my emails" --agent email-agent

# Read prompt from stdin (pipe mode)
echo "Hello" | sera-tui -p -
cat prompt.txt | sera-tui -p - --agent research-agent
```

## Architecture

- HTTP calls use `ureq` (blocking, no async runtime needed)
- WebSocket uses `tungstenite` over a raw `TcpStream` in a background `std::thread`
- WS → TUI events flow via `std::sync::mpsc::channel`
- TUI loop polls crossterm events with a 50ms timeout and drains the mpsc channel on each tick

## Learnings

_(Add TUI-specific discoveries here.)_
