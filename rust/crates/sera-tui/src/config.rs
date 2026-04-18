//! TUI configuration: CLI args + environment overrides.

use clap::Parser;

/// Launch the SERA TUI against a running gateway.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "sera-tui",
    about = "SERA operator terminal UI (ratatui)",
    long_about = "SERA operator terminal UI.\n\
                  \n\
                  Connects to a running `sera-gateway` over HTTP, streams live session\n\
                  events over SSE when available, and exposes agents, sessions, HITL\n\
                  approvals, and evolve proposals on one screen.  All keybindings are\n\
                  configurable via ~/.sera/tui.toml (see docs/tui-config.md)."
)]
pub struct Config {
    /// Gateway base URL (scheme + host + port, no trailing slash).
    #[arg(long, env = "SERA_API_URL", default_value = "http://localhost:8080")]
    pub api_url: String,

    /// API key used for bearer auth.  Leave as the dev default when
    /// running against the autonomous gateway with auth disabled.
    #[arg(long, env = "SERA_API_KEY", default_value = "sera_bootstrap_dev_123")]
    pub api_key: String,

    /// Request timeout in seconds for point-in-time API calls.  The SSE
    /// stream uses its own (longer) keep-alive interval.
    #[arg(long, env = "SERA_TUI_TIMEOUT_SECS", default_value_t = 10)]
    pub timeout_secs: u64,

    /// Poll interval, in milliseconds, for the event loop tick.  Controls
    /// how often background refreshes get a chance to run between key
    /// presses.  Lower = snappier, higher = lower CPU.
    #[arg(long, env = "SERA_TUI_TICK_MS", default_value_t = 250)]
    pub tick_ms: u64,
}

impl Config {
    /// Parse from the process argv.  Thin wrapper so callers don't need
    /// to depend on `clap::Parser` directly.
    #[allow(dead_code)]
    pub fn from_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_from(args: &[&str]) -> Config {
        Config::parse_from(args)
    }

    #[test]
    fn defaults_when_no_flags_given() {
        let cfg = parse_from(&["sera-tui"]);
        assert_eq!(cfg.api_url, "http://localhost:8080");
        assert_eq!(cfg.api_key, "sera_bootstrap_dev_123");
        assert_eq!(cfg.timeout_secs, 10);
        assert_eq!(cfg.tick_ms, 250);
    }

    #[test]
    fn api_url_flag_overrides_default() {
        let cfg = parse_from(&["sera-tui", "--api-url", "http://gateway:9000"]);
        assert_eq!(cfg.api_url, "http://gateway:9000");
    }

    #[test]
    fn timeout_secs_parses_integer() {
        let cfg = parse_from(&["sera-tui", "--timeout-secs", "30"]);
        assert_eq!(cfg.timeout_secs, 30);
    }
}
