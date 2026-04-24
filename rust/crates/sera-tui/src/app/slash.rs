//! Slash-command parser for the TUI composer.
//!
//! When the composer text starts with `/`, [`parse`] converts it to a
//! [`SlashCommand`] variant instead of sending it to the chat API.

/// Commands that can be typed in the TUI composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    /// Clear transcript + tool log and start a fresh turn.
    New,
    /// Switch the active agent by name.
    Agent(String),
    /// Show the help modal with all available commands.
    Help,
    /// Exit the TUI cleanly.
    Quit,
}

/// Parse a `/`-prefixed string into a [`SlashCommand`].
///
/// Returns `Err` with a human-readable message when the command is unknown
/// or has invalid arguments.
pub fn parse(input: &str) -> Result<SlashCommand, String> {
    let trimmed = input.trim();
    let without_slash = trimmed.strip_prefix('/').unwrap_or(trimmed);

    let (cmd, rest) = match without_slash.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (without_slash, ""),
    };

    match cmd {
        "new" | "clear" => Ok(SlashCommand::New),
        "agent" => {
            if rest.is_empty() {
                Err("/agent requires a name: /agent <name>".to_owned())
            } else {
                Ok(SlashCommand::Agent(rest.to_owned()))
            }
        }
        "help" => Ok(SlashCommand::Help),
        "quit" => Ok(SlashCommand::Quit),
        other => Err(format!("unknown command: /{other}  (type /help for list)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_new() {
        assert_eq!(parse("/new"), Ok(SlashCommand::New));
    }

    #[test]
    fn parse_clear_is_alias_for_new() {
        assert_eq!(parse("/clear"), Ok(SlashCommand::New));
    }

    #[test]
    fn parse_agent_with_name() {
        assert_eq!(parse("/agent my-bot"), Ok(SlashCommand::Agent("my-bot".to_owned())));
    }

    #[test]
    fn parse_agent_without_name_is_error() {
        assert!(parse("/agent").is_err());
        assert!(parse("/agent ").is_err());
    }

    #[test]
    fn parse_help() {
        assert_eq!(parse("/help"), Ok(SlashCommand::Help));
    }

    #[test]
    fn parse_quit() {
        assert_eq!(parse("/quit"), Ok(SlashCommand::Quit));
    }

    #[test]
    fn parse_unknown_is_error() {
        let err = parse("/frobnicate");
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("frobnicate"));
    }

    #[test]
    fn parse_trims_whitespace() {
        assert_eq!(parse("  /new  "), Ok(SlashCommand::New));
    }
}
