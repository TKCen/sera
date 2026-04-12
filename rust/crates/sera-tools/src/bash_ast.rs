//! Bash AST checker — blocks unsafe command patterns via hand-rolled tokenizer.

/// Errors detected by the bash AST checker.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BashAstError {
    #[error("backtick command substitution is not allowed")]
    BacktickSubstitution,
    #[error("process substitution $() is not allowed")]
    ProcessSubstitution,
    #[error("unsafe metacharacter injection detected")]
    MetacharInjection,
    #[error("out-of-sandbox filesystem access detected")]
    OutOfSandboxAccess,
}

/// Checks bash commands for unsafe patterns.
pub struct BashAstChecker;

impl BashAstChecker {
    /// Check a command string for unsafe bash constructs.
    ///
    /// Blocks:
    /// - Backtick substitution: `` `cmd` ``
    /// - Process substitution: `$(cmd)`
    /// - Unsafe metacharacters: `;`, `|`, `&`, `>`, `<`, `\n` outside quotes
    pub fn check(command: &str) -> Result<(), BashAstError> {
        let bytes = command.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        while i < len {
            let ch = bytes[i];

            // Handle single-quote toggle (no escapes inside single quotes)
            if ch == b'\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
                i += 1;
                continue;
            }

            // Handle double-quote toggle
            if ch == b'"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                i += 1;
                continue;
            }

            // Skip escape sequences
            if ch == b'\\' && !in_single_quote {
                i += 2;
                continue;
            }

            // Inside single quotes — nothing is special
            if in_single_quote {
                i += 1;
                continue;
            }

            // Backtick substitution
            if ch == b'`' {
                return Err(BashAstError::BacktickSubstitution);
            }

            // Process substitution $(...)
            if ch == b'$' && i + 1 < len && bytes[i + 1] == b'(' {
                return Err(BashAstError::ProcessSubstitution);
            }

            // Unsafe metacharacters outside quotes
            if !in_double_quote {
                match ch {
                    b';' | b'|' | b'&' | b'>' | b'<' | b'\n' => {
                        return Err(BashAstError::MetacharInjection);
                    }
                    _ => {}
                }
            }

            i += 1;
        }

        Ok(())
    }
}
