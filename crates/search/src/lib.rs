//! Fuzzy search index over all six resource kinds, shared by the quick
//! launcher and the full manager. Headless: no I/O, no UI types.
//!
//! [`SearchIndex::search`] is synchronous and cheap enough for typing-rate
//! updates on local data; run it off the UI thread and pair results with a
//! generation so stale results can be dropped. [`reselect`] keeps the
//! user's selection stable when results are re-ranked asynchronously.

mod doc;
mod index;
mod query;
#[doc(hidden)]
pub mod synthetic;

use std::collections::{HashMap, HashSet};

use perkolator_core::ResourceId;

pub use doc::SearchDoc;
pub use index::SearchIndex;
pub use query::{Query, ScopeFilter};

/// Which field class produced a hit's rank. Ordered worst to best.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchTier {
    /// Empty query: listed, not matched.
    Unfiltered = 0,
    Extra = 1,
    Summary = 2,
    /// Tags or publisher.
    Secondary = 3,
    NameFuzzy = 4,
    NamePrefix = 5,
    NameExact = 6,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub id: ResourceId,
    pub score: u32,
    pub tier: MatchTier,
    /// Sorted char indices into the name for highlighting (grapheme indices
    /// as produced by nucleo; identical to char indices for ASCII names).
    pub name_match: Vec<u32>,
}

/// Bounded ranking bonuses. They reorder hits within a match tier and never
/// exclude a match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Boosts {
    /// Recency rank, 0 = most recently used.
    pub recent: HashMap<ResourceId, u32>,
    pub favorites: HashSet<ResourceId>,
}

/// Index to select after `hits` replaced a previous list: the previously
/// selected id if still present, else the first row.
pub fn reselect(previous: Option<&ResourceId>, hits: &[SearchHit]) -> usize {
    previous
        .and_then(|id| hits.iter().position(|hit| &hit.id == id))
        .unwrap_or(0)
}
