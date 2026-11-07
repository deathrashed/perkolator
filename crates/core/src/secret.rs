use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

/// A value that may hold a credential (MCP env vars, headers, tokens).
///
/// `Debug`, `Display` and `Serialize` always redact, so secrets cannot leak
/// into logs, IPC events, caches or debug reports by accident. Reading the
/// raw value requires the explicit [`SecretString::expose`] call.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub const REDACTED: &'static str = "<redacted>";

    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Raw access. Only call for an explicit, user-confirmed operation.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// True when the value is a reference such as `${GITHUB_TOKEN}` or
    /// `env:NAME` rather than a literal credential.
    pub fn is_reference(&self) -> bool {
        let value = self.0.trim();
        (value.starts_with("${") && value.ends_with('}'))
            || value.starts_with("env:")
            || (value.starts_with('$')
                && value.len() > 1
                && value[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    }

    /// Safe display: references are shown verbatim, literals are redacted.
    pub fn display_value(&self) -> &str {
        if self.is_reference() {
            &self.0
        } else {
            Self::REDACTED
        }
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretString({})", self.display_value())
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_value())
    }
}

impl Serialize for SecretString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.display_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_values_never_print() {
        let secret = SecretString::new("ghp_supersecret");
        assert_eq!(format!("{secret}"), SecretString::REDACTED);
        assert!(!format!("{secret:?}").contains("ghp_"));
        assert!(!serde_json::to_string(&secret).unwrap().contains("ghp_"));
        assert_eq!(secret.expose(), "ghp_supersecret");
    }

    #[test]
    fn references_are_displayed() {
        for reference in ["${GITHUB_TOKEN}", "env:API_KEY", "$HOME_DIR"] {
            assert_eq!(SecretString::new(reference).display_value(), reference);
        }
        assert_eq!(
            SecretString::new("$ not ref").display_value(),
            SecretString::REDACTED
        );
    }
}
