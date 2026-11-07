use std::collections::BTreeSet;

use perkolator_core::ResourceKind;

/// Which origins a query covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScopeFilter {
    #[default]
    All,
    /// Library, agent-host and project resources (anything not remote/cached).
    Local,
    /// Resources with at least one installed/enabled/linked location.
    Installed,
    /// Remote catalogue results and cached copies of them.
    Remote,
}

/// A parsed launcher query: leading filter tokens plus free text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    /// Free text passed to the fuzzy matcher. Supports nucleo/fzf atom syntax
    /// (`^prefix`, `suffix$`, `'substring`, `!negation`).
    pub text: String,
    /// Kinds to include; empty means all kinds.
    pub kinds: BTreeSet<ResourceKind>,
    pub scope: ScopeFilter,
}

impl Query {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Parse leading filter tokens, then treat the remainder as text.
    ///
    /// Kind prefixes: `skill:`, `mcp:`, `agent:`, `prompt:`, `rule:`,
    /// `plugin:` (and plurals / `mcp-server:`), either followed by a space or
    /// glued to the first word (`mcp:git`). Scope tokens: `@installed`,
    /// `@local`, `@remote`. Parsing stops at the first ordinary token, so
    /// `notes: skill:` searches for that literal text.
    pub fn parse(input: &str) -> Query {
        let mut query = Query::default();
        let mut rest = input.trim_start();
        loop {
            let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let token = &rest[..token_end];
            if token.is_empty() {
                break;
            }
            if let Some(scope) = parse_scope(token) {
                query.scope = scope;
                rest = rest[token_end..].trim_start();
                continue;
            }
            if let Some((prefix, glued)) = token.split_once(':')
                && let Ok(kind) = prefix.to_ascii_lowercase().parse::<ResourceKind>()
            {
                query.kinds.insert(kind);
                // `mcp:git` keeps `git` as the start of the text.
                let consumed = token_end - glued.len();
                rest = rest[consumed..].trim_start();
                continue;
            }
            break;
        }
        query.text = rest.trim_end().to_owned();
        query
    }

    pub fn allows_kind(&self, kind: ResourceKind) -> bool {
        self.kinds.is_empty() || self.kinds.contains(&kind)
    }
}

fn parse_scope(token: &str) -> Option<ScopeFilter> {
    match token.to_ascii_lowercase().as_str() {
        "@installed" => Some(ScopeFilter::Installed),
        "@local" => Some(ScopeFilter::Local),
        "@remote" => Some(ScopeFilter::Remote),
        "@all" => Some(ScopeFilter::All),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(list: &[ResourceKind]) -> BTreeSet<ResourceKind> {
        list.iter().copied().collect()
    }

    #[test]
    fn kind_prefix_with_space() {
        let query = Query::parse("mcp: git");
        assert_eq!(query.kinds, kinds(&[ResourceKind::McpServer]));
        assert_eq!(query.text, "git");
        assert_eq!(query.scope, ScopeFilter::All);
    }

    #[test]
    fn glued_prefix_plurals_and_scope() {
        let query = Query::parse("@installed skills:front end");
        assert_eq!(query.kinds, kinds(&[ResourceKind::Skill]));
        assert_eq!(query.scope, ScopeFilter::Installed);
        assert_eq!(query.text, "front end");
    }

    #[test]
    fn multiple_prefixes_accumulate() {
        let query = Query::parse("rule: Prompt: @remote review");
        assert_eq!(
            query.kinds,
            kinds(&[ResourceKind::Rule, ResourceKind::Prompt])
        );
        assert_eq!(query.scope, ScopeFilter::Remote);
        assert_eq!(query.text, "review");
    }

    #[test]
    fn unknown_prefix_is_text() {
        let query = Query::parse("model: gpt skill:");
        assert!(query.kinds.is_empty());
        assert_eq!(query.text, "model: gpt skill:");
        let url = Query::parse("https://example.invalid");
        assert!(url.kinds.is_empty());
        assert_eq!(url.text, "https://example.invalid");
    }

    #[test]
    fn prefix_only_has_empty_text() {
        let query = Query::parse("  plugin:  ");
        assert_eq!(query.kinds, kinds(&[ResourceKind::Plugin]));
        assert_eq!(query.text, "");
    }
}
