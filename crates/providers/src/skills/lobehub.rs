//! `lobehub`: LobeHub Market through the `lhm` CLI (`@lobehub/market-cli`,
//! verified against 0.0.46). Search runs `lhm skills search` and
//! `lhm mcp search` with `--output json`, which print the Market SDK list
//! response verbatim. Both require M2M credentials created by
//! `lhm register`; Perkolator never runs registration or login itself.
//! The CLI has no agent or plugin-catalogue search, so those kinds are not
//! declared.

use std::time::Duration;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::github::RepoRef;
use perkolator_marketplace::{
    AuthIntent, AuthStatus, AuthStrategy, CommandOutput, MarketplaceProvider, Pagination,
    ProviderCapabilities, ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions,
    RemoteItem, SearchPage, SearchRequest, Transport, now_unix,
};
use serde_json::Value;

use super::common::{ensure_active, provider_id};

pub const ID: &str = "lobehub";
pub const BINARY: &str = "lhm";
/// Node start-up plus one Market API round trip.
pub const CLI_TIMEOUT: Duration = Duration::from_secs(20);
/// Upper bound for one CLI page.
const MAX_PAGE_SIZE: usize = 100;

pub struct LobeHub {
    descriptor: ProviderDescriptor,
}

impl LobeHub {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "LobeHub Market".into(),
                homepage: "https://market.lobehub.com".into(),
                docs_url: Some("https://www.npmjs.com/package/@lobehub/market-cli".into()),
                kinds: vec![ResourceKind::Skill, ResourceKind::McpServer],
                transport: Transport::Cli,
                auth: AuthStrategy::ExternalCli {
                    binary: BINARY.into(),
                },
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: false,
                    fetch_files: false,
                    download: false,
                    install: false,
                    pagination: Pagination::Page,
                    categories: true,
                    rate_limit_policy: Some(
                        "Market API quotas apply per M2M client; `lhm register` is itself rate limited".into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "Needs the `lhm` CLI (Node 22+) on PATH. Perkolator never installs it.".into(),
                    "Search needs M2M credentials from `lhm register`; `lhm login` (user OAuth) alone does not enable `lhm skills search`.".into(),
                    "LobeHub agents and plugins are not searchable through lhm 0.0.46, so only skills and MCP servers are listed.".into(),
                ],
            },
        }
    }
}

impl Default for LobeHub {
    fn default() -> Self {
        Self::new()
    }
}

/// CLI subcommand, the kind it lists and the upstream id prefix.
const CATALOGUES: [(&str, ResourceKind, &str); 2] = [
    ("skills", ResourceKind::Skill, "skill"),
    ("mcp", ResourceKind::McpServer, "mcp"),
];

fn parse_page(cursor: Option<&str>) -> Result<u64, ProviderError> {
    match cursor {
        None => Ok(1),
        Some(cursor) => cursor
            .parse::<u64>()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| ProviderError::malformed(format!("invalid cursor `{cursor}`"))),
    }
}

pub(crate) fn search_args(subcommand: &str, request: &SearchRequest, page: u64) -> Vec<String> {
    let mut args = vec![subcommand.to_owned(), "search".to_owned()];
    let query = request.query.trim();
    if !query.is_empty() {
        // `--q=<value>` keeps a query starting with `-` from parsing as a flag.
        args.push(format!("--q={query}"));
    }
    if let Some(category) = request.category.as_deref() {
        args.push(format!("--category={category}"));
    }
    args.push(format!("--page={page}"));
    args.push(format!(
        "--page-size={}",
        request.page_size.clamp(1, MAX_PAGE_SIZE)
    ));
    args.push("--output=json".to_owned());
    args
}

/// Map a failed run without echoing stderr (it may quote credentials).
fn cli_failure(output: &CommandOutput) -> ProviderError {
    let stderr = output.stderr.to_ascii_lowercase();
    if stderr.contains("no credentials found")
        || stderr.contains("unauthorized")
        || stderr.contains("401")
    {
        ProviderError::NeedsAuth
    } else if stderr.contains("forbidden") || stderr.contains("403") {
        ProviderError::Forbidden
    } else if stderr.contains("rate limit") || stderr.contains("429") {
        ProviderError::RateLimited {
            retry_after_secs: None,
        }
    } else {
        ProviderError::Network {
            message: match output.status {
                Some(code) => format!("`{BINARY}` exited with status {code}"),
                None => format!("`{BINARY}` was terminated"),
            },
        }
    }
}

fn to_item(
    entry: &Value,
    kind: ResourceKind,
    id_prefix: &str,
    fetched_at_unix: u64,
) -> Option<RemoteItem> {
    let identifier = entry["identifier"].as_str()?;
    let title = entry["name"].as_str().unwrap_or(identifier).to_owned();
    let summary = entry["description"]
        .as_str()
        .or_else(|| entry["summary"].as_str())
        .unwrap_or_default()
        .to_owned();
    let repo = entry["github"]["url"].as_str().and_then(RepoRef::parse);
    let homepage = entry["homepage"]
        .as_str()
        .filter(|url| url.starts_with("https://"))
        .map(str::to_owned);
    Some(RemoteItem {
        source: provider_id(ID),
        upstream_id: format!("{id_prefix}:{identifier}"),
        kind,
        title,
        summary,
        canonical_url: homepage.or_else(|| repo.as_ref().map(RepoRef::html_url)),
        repo_url: repo.as_ref().map(RepoRef::html_url),
        resource_path: None,
        package_ref: Some(identifier.to_owned()),
        version: entry["version"].as_str().map(str::to_owned),
        author: entry["author"].as_str().map(str::to_owned),
        categories: entry["category"]
            .as_str()
            .map(str::to_owned)
            .into_iter()
            .collect(),
        actions: RemoteActions {
            view_source: repo.is_some(),
            fetch_files: false,
            install: false,
        },
        fetched_at_unix,
    })
}

/// Parsed list response: items, whether a later page exists, total count.
pub(crate) fn parse_list(
    stdout: &str,
    kind: ResourceKind,
    id_prefix: &str,
) -> Result<(Vec<RemoteItem>, bool, Option<u64>), ProviderError> {
    let json: Value = serde_json::from_str(stdout)
        .map_err(|error| ProviderError::malformed(format!("invalid JSON from lhm: {error}")))?;
    let entries = json["items"]
        .as_array()
        .ok_or_else(|| ProviderError::malformed("lhm output without `items`"))?;
    let fetched_at_unix = now_unix();
    let items = entries
        .iter()
        .filter_map(|entry| to_item(entry, kind, id_prefix, fetched_at_unix))
        .collect();
    let has_more = match (json["currentPage"].as_u64(), json["totalPages"].as_u64()) {
        (Some(current), Some(pages)) => current < pages,
        _ => false,
    };
    Ok((items, has_more, json["totalCount"].as_u64()))
}

impl MarketplaceProvider for LobeHub {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    /// Only PATH detection: `lhm auth status` contacts the Market API.
    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        if env.commands.which(BINARY).is_some() {
            AuthStatus::Unknown
        } else {
            AuthStatus::CliMissing {
                binary: BINARY.into(),
            }
        }
    }

    fn connect_intent(&self) -> AuthIntent {
        AuthIntent::Unsupported {
            reason: "Search uses LobeHub M2M credentials: run `lhm register --name Perkolator` once in a terminal (registration is rate limited; do not repeat it). `lhm login` alone does not enable search.".into(),
        }
    }

    fn search(
        &self,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError> {
        let catalogues: Vec<_> = CATALOGUES
            .iter()
            .filter(|(_, kind, _)| request.wants(*kind))
            .collect();
        if catalogues.is_empty() {
            return Ok(SearchPage::empty());
        }
        let page = parse_page(request.cursor.as_deref())?;
        let program = env
            .commands
            .which(BINARY)
            .ok_or_else(|| ProviderError::CliMissing {
                binary: BINARY.into(),
            })?;
        let mut items = Vec::new();
        let mut any_more = false;
        let mut total = Some(0u64);
        for (subcommand, kind, id_prefix) in catalogues {
            ensure_active(cancel)?;
            let args = search_args(subcommand, request, page);
            let output = env.commands.run(&program, &args, CLI_TIMEOUT)?;
            ensure_active(cancel)?;
            if output.status != Some(0) {
                return Err(cli_failure(&output));
            }
            let (found, has_more, count) = parse_list(&output.stdout, *kind, id_prefix)?;
            items.extend(found);
            any_more |= has_more;
            total = total.zip(count).map(|(sum, count)| sum + count);
        }
        Ok(SearchPage {
            items,
            next_cursor: any_more.then(|| (page + 1).to_string()),
            total,
        })
    }
}
