use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use perkolator_core::{
    CancelToken, Capability, CapabilitySet, InstallState, LocationRole, Provenance, ProviderId,
    ResourceDetails, ResourceId, ResourceKind, ResourceLocation, ResourceRecord, Scope, SourceKind,
    ValidationIssue,
};
use walkdir::WalkDir;

use crate::files::is_skipped_dir;
use crate::kinds::{agents, mcp, plugins, prompts, rules, skills};
use crate::{
    BrokenLink, Registry, RootLayout, ScanConfig, ScanLimits, ScanProblem, ScanReport, ScanRoot,
};

/// Scan every configured root. Synchronous and read-only: run it on a
/// background thread. Cancellation is checked between entries; a cancelled
/// scan returns the partial report with `cancelled = true`.
pub fn scan(config: &ScanConfig, cancel: &CancelToken) -> ScanReport {
    let mut scanner = Scanner::new(&config.limits, cancel);
    for root in &config.roots {
        if cancel.is_cancelled() {
            break;
        }
        scanner.scan_root(root);
    }
    if !cancel.is_cancelled() {
        skills::resolve_links(&mut scanner);
    }
    scanner.finish()
}

/// A symlinked skill directory in an agent root, resolved once every root
/// has been scanned so it can attach to a library record.
pub(crate) struct PendingSkillLink {
    pub link: PathBuf,
    pub target: PathBuf,
    pub host: String,
    pub scope: Scope,
    pub provider: ProviderId,
    pub source: SourceKind,
}

pub(crate) struct Scanner<'a> {
    pub limits: &'a ScanLimits,
    pub cancel: &'a CancelToken,
    pub registry: Registry,
    pub problems: Vec<ScanProblem>,
    pub broken_links: Vec<BrokenLink>,
    pub pending_links: Vec<PendingSkillLink>,
    /// Canonical path of every file-backed record, so the same physical
    /// resource reached from two roots becomes one record.
    by_canonical: HashMap<(ResourceKind, PathBuf), ResourceId>,
}

impl<'a> Scanner<'a> {
    fn new(limits: &'a ScanLimits, cancel: &'a CancelToken) -> Self {
        Self {
            limits,
            cancel,
            registry: Registry::new(),
            problems: Vec::new(),
            broken_links: Vec::new(),
            pending_links: Vec::new(),
            by_canonical: HashMap::new(),
        }
    }

    fn scan_root(&mut self, root: &ScanRoot) {
        match &root.layout {
            RootLayout::SkillLibrary => skills::scan_library(self, root),
            RootLayout::SkillDirectory { host } => skills::scan_directory(self, root, host),
            RootLayout::McpConfig { client, format } => {
                mcp::scan_config(self, root, client, *format)
            }
            RootLayout::AgentProfiles { host } => agents::scan(self, root, host),
            RootLayout::Prompts { host } => prompts::scan(self, root, host.as_deref()),
            RootLayout::RuleFile {
                host,
                format,
                scope,
            } => rules::scan_file(self, root, host.as_deref(), *format, scope),
            RootLayout::RuleDirectory { host, scope } => {
                rules::scan_directory(self, root, host.as_deref(), scope)
            }
            RootLayout::Plugins { host } => plugins::scan(self, root, host),
        }
    }

    fn finish(self) -> ScanReport {
        ScanReport {
            records: self.registry.into_records(),
            problems: self.problems,
            broken_links: self.broken_links,
            cancelled: self.cancel.is_cancelled(),
        }
    }

    pub fn problem(&mut self, path: impl Into<PathBuf>, message: impl Into<String>) {
        self.problems.push(ScanProblem::new(path, message));
    }

    pub fn broken_link(&mut self, path: &Path, host: Option<&str>) {
        let target = fs::read_link(path).unwrap_or_default();
        self.problem(
            path,
            format!(
                "broken symlink: target `{}` does not exist",
                target.display()
            ),
        );
        self.broken_links.push(BrokenLink {
            path: path.to_path_buf(),
            target,
            host: host.map(str::to_owned),
        });
    }

    /// Record already known for a physical path.
    pub fn known(&self, kind: ResourceKind, canonical: &Path) -> Option<ResourceId> {
        self.by_canonical
            .get(&(kind, canonical.to_path_buf()))
            .cloned()
    }

    /// Insert a record, merging into an existing record for the same
    /// canonical path when one exists (even if reached via another root).
    pub fn upsert(&mut self, canonical: Option<&Path>, record: ResourceRecord) {
        if let Some(canonical) = canonical {
            let key = (record.kind(), canonical.to_path_buf());
            if let Some(id) = self.by_canonical.get(&key).cloned() {
                self.registry.merge_into_id(&id, record);
                return;
            }
            self.by_canonical.insert(key, record.id.clone());
        }
        self.registry.insert(record);
    }

    /// Documents below `dir` with one of `extensions` (recursive, bounded,
    /// never through symlinked directories). Symlinked files are only
    /// checked for existence.
    pub fn document_files(
        &mut self,
        dir: &Path,
        extensions: &[&str],
        host: Option<&str>,
    ) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let matches_extension = |path: &Path| {
            path.extension()
                .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
                .is_some_and(|extension| extensions.contains(&extension.as_str()))
        };
        let walker = WalkDir::new(dir)
            .follow_links(false)
            .min_depth(1)
            .max_depth(self.limits.max_depth)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !(entry.file_type().is_dir() && (name.starts_with('.') || is_skipped_dir(&name)))
            });
        for entry in walker {
            if self.cancel.is_cancelled() {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    let path = error.path().unwrap_or(dir).to_path_buf();
                    self.problem(path, format!("cannot list: {error}"));
                    continue;
                }
            };
            let path = entry.path();
            if !matches_extension(path) {
                continue;
            }
            if entry.file_type().is_symlink() {
                match fs::metadata(path) {
                    Ok(metadata) if metadata.is_file() => found.push(path.to_path_buf()),
                    Ok(_) => {}
                    Err(_) => self.broken_link(path, host),
                }
            } else if entry.file_type().is_file() {
                found.push(path.to_path_buf());
            }
        }
        found
    }
}

/// Scope implied by where a root was found.
pub(crate) fn scope_for(source: &SourceKind) -> Scope {
    match source {
        SourceKind::AgentHost { scope, .. } => scope.clone(),
        SourceKind::Project { root } => Scope::Project { root: root.clone() },
        _ => Scope::Global,
    }
}

pub(crate) fn key_for(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Locations for a single-file resource: canonical, plus a linked
/// deployment when it was reached through a symlink in a host directory.
pub(crate) fn file_locations(
    found: &Path,
    canonical: &Path,
    host: Option<&str>,
    scope: &Scope,
) -> Vec<ResourceLocation> {
    let mut locations = vec![ResourceLocation {
        path: canonical.to_path_buf(),
        role: LocationRole::Canonical,
        state: InstallState::Installed,
    }];
    if found != canonical
        && let Some(host) = host
    {
        locations.push(ResourceLocation {
            path: found.to_path_buf(),
            role: LocationRole::Deployed {
                target: host.to_owned(),
                scope: scope.clone(),
            },
            state: InstallState::Linked,
        });
    }
    locations
}

/// Fields every newly built record needs.
pub(crate) struct RecordDraft {
    pub id: ResourceId,
    pub name: String,
    pub summary: String,
    pub details: ResourceDetails,
    pub source: SourceKind,
    pub locations: Vec<ResourceLocation>,
}

/// Optional metadata, only ever filled from real source data.
#[derive(Default)]
pub(crate) struct RecordMeta {
    pub publisher: Option<String>,
    pub tags: Vec<String>,
    pub version: Option<String>,
    pub origin_url: Option<String>,
    pub fingerprint: Option<String>,
    pub issues: Vec<ValidationIssue>,
}

pub(crate) fn build_record(draft: RecordDraft, meta: RecordMeta) -> ResourceRecord {
    let mut capabilities = CapabilitySet::for_details(&draft.details);
    if meta.origin_url.is_some() {
        capabilities.insert(Capability::SourceUrl);
    }
    ResourceRecord {
        id: draft.id,
        name: draft.name,
        summary: draft.summary,
        publisher: meta.publisher,
        tags: meta.tags,
        version: meta.version,
        provenance: Provenance {
            source: draft.source,
            origin_url: meta.origin_url,
            fingerprint: meta.fingerprint,
        },
        locations: draft.locations,
        capabilities,
        details: draft.details,
        issues: meta.issues,
    }
}
