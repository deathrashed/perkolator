use std::fmt;
use std::io;

/// Failures that stop a command before it produced its normal output.
/// "The plan has conflicts" or "validation found errors" are not errors:
/// those are reported in [`crate::CommandOutput`] with `success == false`.
#[derive(Debug)]
pub enum CliError {
    /// Bad arguments or an operation that does not apply to the resource.
    Usage(String),
    NotFound {
        query: String,
        suggestions: Vec<String>,
    },
    Ambiguous {
        query: String,
        candidates: Vec<String>,
    },
    Io {
        context: String,
        error: io::Error,
    },
    /// Error reported by a library crate (targets, deployment, actions).
    Failed(String),
}

impl CliError {
    pub fn usage(message: impl Into<String>) -> Self {
        CliError::Usage(message.into())
    }

    pub fn io(context: impl Into<String>, error: io::Error) -> Self {
        CliError::Io {
            context: context.into(),
            error,
        }
    }

    pub fn failed(error: impl fmt::Display) -> Self {
        CliError::Failed(error.to_string())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(message) | CliError::Failed(message) => f.write_str(message),
            CliError::NotFound { query, suggestions } => {
                write!(f, "no resource matches `{query}`")?;
                if !suggestions.is_empty() {
                    write!(f, "; closest matches:")?;
                    for suggestion in suggestions {
                        write!(f, "\n  {suggestion}")?;
                    }
                }
                Ok(())
            }
            CliError::Ambiguous { query, candidates } => {
                write!(
                    f,
                    "`{query}` matches {} resources; pass one of these ids:",
                    candidates.len()
                )?;
                for candidate in candidates {
                    write!(f, "\n  {candidate}")?;
                }
                Ok(())
            }
            CliError::Io { context, error } => write!(f, "{context}: {error}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::Io { error, .. } => Some(error),
            _ => None,
        }
    }
}
