use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use perkolator_core::{ResourceId, ResourceKind, ResourceLocation, ResourceRecord};

use crate::ScanReport;

/// Whether [`Registry::insert`] added a new record or merged into one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    Merged,
}

/// In-memory set of normalised records keyed by [`ResourceId`], preserving
/// insertion order. Records with the same display name but different IDs
/// stay distinct.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    records: Vec<ResourceRecord>,
    index: HashMap<ResourceId, usize>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_report(report: &ScanReport) -> Self {
        Self::from_records(report.records.iter().cloned())
    }

    pub fn from_records(records: impl IntoIterator<Item = ResourceRecord>) -> Self {
        let mut registry = Self::new();
        for record in records {
            registry.insert(record);
        }
        registry
    }

    /// Insert a record, or merge it into the existing record with the same ID
    /// (locations, issues, tags and capabilities are unioned; existing scalar
    /// metadata wins and gaps are filled from the new record).
    pub fn insert(&mut self, record: ResourceRecord) -> InsertOutcome {
        if let Some(&index) = self.index.get(&record.id) {
            merge_into(&mut self.records[index], record);
            InsertOutcome::Merged
        } else {
            self.index.insert(record.id.clone(), self.records.len());
            self.records.push(record);
            InsertOutcome::Inserted
        }
    }

    /// Merge `other` into the existing record `id`, ignoring `other.id`.
    /// Returns false when `id` is unknown.
    pub(crate) fn merge_into_id(&mut self, id: &ResourceId, other: ResourceRecord) -> bool {
        match self.index.get(id) {
            Some(&index) => {
                merge_into(&mut self.records[index], other);
                true
            }
            None => false,
        }
    }

    /// Add a location to record `id` unless an identical path/role exists.
    pub(crate) fn add_location(&mut self, id: &ResourceId, location: ResourceLocation) -> bool {
        match self.index.get(id) {
            Some(&index) => {
                push_location(&mut self.records[index], location);
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: &ResourceId) -> Option<&ResourceRecord> {
        self.index.get(id).map(|&index| &self.records[index])
    }

    pub fn iter(&self) -> impl Iterator<Item = &ResourceRecord> {
        self.records.iter()
    }

    pub fn by_kind(&self, kind: ResourceKind) -> impl Iterator<Item = &ResourceRecord> {
        self.records
            .iter()
            .filter(move |record| record.kind() == kind)
    }

    /// Records that have any location at exactly `path`.
    pub fn find_by_path<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a ResourceRecord> {
        self.records.iter().filter(move |record| {
            record
                .locations
                .iter()
                .any(|location| location.path == path)
        })
    }

    pub fn count_by_kind(&self) -> BTreeMap<ResourceKind, usize> {
        let mut counts = BTreeMap::new();
        for record in &self.records {
            *counts.entry(record.kind()).or_insert(0) += 1;
        }
        counts
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn into_records(self) -> Vec<ResourceRecord> {
        self.records
    }
}

fn push_location(existing: &mut ResourceRecord, location: ResourceLocation) {
    let duplicate = existing
        .locations
        .iter()
        .any(|known| known.path == location.path && known.role == location.role);
    if !duplicate {
        existing.locations.push(location);
    }
}

fn merge_into(existing: &mut ResourceRecord, other: ResourceRecord) {
    for location in other.locations {
        push_location(existing, location);
    }
    for issue in other.issues {
        if !existing.issues.contains(&issue) {
            existing.issues.push(issue);
        }
    }
    for tag in other.tags {
        if !existing.tags.contains(&tag) {
            existing.tags.push(tag);
        }
    }
    for capability in other.capabilities.iter() {
        existing.capabilities.insert(capability);
    }
    if existing.summary.is_empty() {
        existing.summary = other.summary;
    }
    existing.version = existing.version.take().or(other.version);
    existing.publisher = existing.publisher.take().or(other.publisher);
    if existing.provenance.origin_url.is_none() {
        existing.provenance.origin_url = other.provenance.origin_url;
    }
    if existing.provenance.fingerprint.is_none() {
        existing.provenance.fingerprint = other.provenance.fingerprint;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use perkolator_core::{
        AgentProfileDetails, CapabilitySet, InstallState, LocationRole, Provenance, ProviderId,
        ResourceDetails, ResourceLocation, SourceKind,
    };

    use super::*;

    fn record(provider: &str, path: &str) -> ResourceRecord {
        let details = ResourceDetails::Agent(AgentProfileDetails {
            host: None,
            file: PathBuf::from(path),
            model: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
        });
        ResourceRecord {
            id: ResourceId::new(
                ResourceKind::Agent,
                ProviderId::new(provider).unwrap(),
                path,
            ),
            name: "reviewer".into(),
            summary: String::new(),
            publisher: None,
            tags: vec!["a".into()],
            version: None,
            provenance: Provenance {
                source: SourceKind::Library {
                    root: PathBuf::from("/"),
                },
                origin_url: None,
                fingerprint: None,
            },
            locations: vec![ResourceLocation {
                path: PathBuf::from(path),
                role: LocationRole::Canonical,
                state: InstallState::Installed,
            }],
            capabilities: CapabilitySet::for_details(&details),
            details,
            issues: vec![],
        }
    }

    #[test]
    fn same_id_merges_and_dedupes_locations() {
        let mut registry = Registry::new();
        assert_eq!(
            registry.insert(record("library", "/a.md")),
            InsertOutcome::Inserted
        );
        let mut again = record("library", "/a.md");
        again.summary = "filled".into();
        again.tags.push("b".into());
        again.locations.push(ResourceLocation {
            path: PathBuf::from("/link.md"),
            role: LocationRole::Canonical,
            state: InstallState::Linked,
        });
        assert_eq!(registry.insert(again), InsertOutcome::Merged);
        assert_eq!(registry.len(), 1);
        let merged = registry.iter().next().unwrap();
        assert_eq!(merged.locations.len(), 2);
        assert_eq!(merged.tags, vec!["a", "b"]);
        assert_eq!(merged.summary, "filled");
    }

    #[test]
    fn same_name_different_ids_stay_distinct() {
        let registry =
            Registry::from_records([record("library", "/a.md"), record("cursor", "/a.md")]);
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.by_kind(ResourceKind::Agent).count(), 2);
        assert_eq!(registry.find_by_path(Path::new("/a.md")).count(), 2);
        assert_eq!(registry.count_by_kind()[&ResourceKind::Agent], 2);
    }
}
