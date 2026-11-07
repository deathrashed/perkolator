use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, TryLockError};

use nucleo_matcher::pattern::{Atom, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};
use perkolator_core::{ResourceId, ResourceRecord};

use crate::{Boosts, MatchTier, Query, ScopeFilter, SearchDoc, SearchHit};

/// Width of one match-quality tier in the score space. Raw matcher scores and
/// boosts are clamped below this, so a better tier always outranks a worse
/// one regardless of boosts.
const TIER_SPAN: u32 = 1_000_000;
const BOOST_INSTALLED: u32 = 120;
const BOOST_FAVORITE: u32 = 400;
const BOOST_RECENT_MAX: u32 = 300;
/// Recency bonus lost per rank step (rank 0 = most recent; 20+ = no bonus).
const BOOST_RECENT_STEP: u32 = 15;
const MAX_BOOST: u32 = BOOST_INSTALLED + BOOST_FAVORITE + BOOST_RECENT_MAX;
const RAW_CAP: u32 = TIER_SPAN - MAX_BOOST - 1;
/// Name-prefix matches rank shorter names first: `PREFIX_RAW - name length`.
const PREFIX_RAW: u32 = 10_000;
/// Long text fields are truncated for matching; previews show full text.
const MAX_SUMMARY_CHARS: usize = 512;
const MAX_EXTRA_CHARS: usize = 1024;

/// Pre-segmented haystacks for one document.
struct Entry {
    doc: SearchDoc,
    id_key: String,
    name_key: String,
    name_chars: u32,
    name: Utf32String,
    secondary: Utf32String,
    summary: Utf32String,
    extra: Utf32String,
}

impl Entry {
    fn new(doc: SearchDoc) -> Self {
        let mut secondary = doc.tags.join(" ");
        if let Some(publisher) = &doc.publisher {
            secondary.push(' ');
            secondary.push_str(publisher);
        }
        let extra = doc.extra.join(" ");
        Self {
            id_key: doc.id.to_string(),
            name_key: normalize(&doc.name),
            name_chars: u32::try_from(doc.name.chars().count()).unwrap_or(u32::MAX),
            name: Utf32String::from(doc.name.as_str()),
            secondary: Utf32String::from(secondary.trim()),
            summary: Utf32String::from(truncate_chars(&doc.summary, MAX_SUMMARY_CHARS)),
            extra: Utf32String::from(truncate_chars(&extra, MAX_EXTRA_CHARS)),
            doc,
        }
    }

    /// Fields in descending weight order.
    fn weighted_fields(&self) -> [(MatchTier, &Utf32String); 4] {
        [
            (MatchTier::NameFuzzy, &self.name),
            (MatchTier::Secondary, &self.secondary),
            (MatchTier::Summary, &self.summary),
            (MatchTier::Extra, &self.extra),
        ]
    }

    fn passes(&self, query: &Query) -> bool {
        query.allows_kind(self.doc.kind)
            && match query.scope {
                ScopeFilter::All => true,
                ScopeFilter::Local => !self.doc.remote,
                ScopeFilter::Installed => self.doc.installed,
                ScopeFilter::Remote => self.doc.remote,
            }
    }

    fn boost(&self, boosts: &Boosts) -> u32 {
        let mut bonus = 0;
        if self.doc.installed {
            bonus += BOOST_INSTALLED;
        }
        if boosts.favorites.contains(&self.doc.id) {
            bonus += BOOST_FAVORITE;
        }
        if let Some(rank) = boosts.recent.get(&self.doc.id) {
            bonus += BOOST_RECENT_MAX.saturating_sub(rank.saturating_mul(BOOST_RECENT_STEP));
        }
        bonus
    }
}

/// Query text compiled once per search.
struct Prepared {
    text_key: String,
    positive: Vec<Atom>,
    negative: Vec<Atom>,
}

impl Prepared {
    fn new(text: &str) -> Self {
        let pattern = Pattern::parse(text, CaseMatching::Ignore, Normalization::Smart);
        let (negative, positive) = pattern.atoms.into_iter().partition(|atom| atom.negative);
        Self {
            text_key: normalize(text),
            positive,
            negative,
        }
    }
}

/// In-memory fuzzy index over every resource kind.
///
/// Matching is synchronous on the caller's thread using `nucleo-matcher`.
/// The threaded `nucleo` crate is unnecessary at Perkolator's scale (see
/// `examples/bench_search.rs`): callers run `search` on a background
/// executor and drop stale results by generation. One reusable [`Matcher`]
/// (which pre-allocates ~135 KB of scratch space) lives behind a `Mutex`, so
/// the index is `Sync`; a concurrent search that finds it busy uses a
/// temporary matcher instead of waiting.
pub struct SearchIndex {
    entries: Vec<Entry>,
    positions: HashMap<ResourceId, usize>,
    matcher: Mutex<Matcher>,
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchIndex")
            .field("len", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl SearchIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            positions: HashMap::new(),
            matcher: Mutex::new(Matcher::new(Config::DEFAULT)),
        }
    }

    pub fn from_records<'a>(records: impl IntoIterator<Item = &'a ResourceRecord>) -> Self {
        Self::from_docs(records.into_iter().map(SearchDoc::from_record))
    }

    pub fn from_docs(docs: impl IntoIterator<Item = SearchDoc>) -> Self {
        let mut index = Self::new();
        for doc in docs {
            index.upsert(doc);
        }
        index
    }

    /// Insert or replace the document with the same id.
    pub fn upsert(&mut self, doc: SearchDoc) {
        let entry = Entry::new(doc);
        match self.positions.get(&entry.doc.id) {
            Some(&position) => self.entries[position] = entry,
            None => {
                self.positions
                    .insert(entry.doc.id.clone(), self.entries.len());
                self.entries.push(entry);
            }
        }
    }

    pub fn remove(&mut self, id: &ResourceId) -> Option<SearchDoc> {
        let position = self.positions.remove(id)?;
        let removed = self.entries.swap_remove(position);
        if let Some(moved) = self.entries.get(position) {
            self.positions.insert(moved.doc.id.clone(), position);
        }
        Some(removed.doc)
    }

    pub fn get(&self, id: &ResourceId) -> Option<&SearchDoc> {
        self.positions
            .get(id)
            .map(|&position| &self.entries[position].doc)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Ranked hits, best first, at most `limit`.
    ///
    /// Empty text lists every document that passes the filters: installed
    /// first, then kind order, then name. Otherwise documents are ranked by
    /// match tier (name exact > name prefix > name fuzzy > tags/publisher >
    /// summary > extra), then raw matcher score plus bounded boosts, with a
    /// deterministic tie-break on name and id. Boosts only reorder within a
    /// tier and never remove a match.
    pub fn search(&self, query: &Query, boosts: &Boosts, limit: usize) -> Vec<SearchHit> {
        if limit == 0 {
            return Vec::new();
        }
        let text = query.text.trim();
        if text.is_empty() {
            return self.list_all(query, boosts, limit);
        }
        let prepared = Prepared::new(text);
        self.with_matcher(|matcher| {
            let mut scored: Vec<(u32, MatchTier, usize)> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.passes(query))
                .filter_map(|(position, entry)| {
                    let (tier, raw) = score_entry(entry, &prepared, matcher)?;
                    let score = tier as u32 * TIER_SPAN + raw.min(RAW_CAP) + entry.boost(boosts);
                    Some((score, tier, position))
                })
                .collect();
            let compare = |a: &(u32, MatchTier, usize), b: &(u32, MatchTier, usize)| {
                b.0.cmp(&a.0)
                    .then_with(|| self.tie_break(&self.entries[a.2], &self.entries[b.2]))
            };
            if scored.len() > limit {
                scored.select_nth_unstable_by(limit - 1, compare);
                scored.truncate(limit);
            }
            scored.sort_unstable_by(compare);
            scored
                .into_iter()
                .map(|(score, tier, position)| {
                    let entry = &self.entries[position];
                    SearchHit {
                        id: entry.doc.id.clone(),
                        score,
                        tier,
                        name_match: name_indices(entry, &prepared, matcher),
                    }
                })
                .collect()
        })
    }

    fn list_all(&self, query: &Query, boosts: &Boosts, limit: usize) -> Vec<SearchHit> {
        let mut entries: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.passes(query))
            .collect();
        entries.sort_unstable_by(|a, b| {
            b.doc
                .installed
                .cmp(&a.doc.installed)
                .then_with(|| a.doc.kind.cmp(&b.doc.kind))
                .then_with(|| self.tie_break(a, b))
        });
        entries
            .into_iter()
            .take(limit)
            .map(|entry| SearchHit {
                id: entry.doc.id.clone(),
                score: entry.boost(boosts),
                tier: MatchTier::Unfiltered,
                name_match: Vec::new(),
            })
            .collect()
    }

    fn tie_break(&self, a: &Entry, b: &Entry) -> Ordering {
        a.name_key
            .cmp(&b.name_key)
            .then_with(|| a.doc.name.cmp(&b.doc.name))
            .then_with(|| a.id_key.cmp(&b.id_key))
    }

    fn with_matcher<R>(&self, run: impl FnOnce(&mut Matcher) -> R) -> R {
        match self.matcher.try_lock() {
            Ok(mut matcher) => run(&mut matcher),
            Err(TryLockError::Poisoned(poisoned)) => run(&mut poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => run(&mut Matcher::new(Config::DEFAULT)),
        }
    }
}

/// Returns the document's tier and raw score, or `None` when any positive
/// atom matches no field or any negative atom matches some field.
fn score_entry(
    entry: &Entry,
    prepared: &Prepared,
    matcher: &mut Matcher,
) -> Option<(MatchTier, u32)> {
    for atom in &prepared.negative {
        for (_, haystack) in entry.weighted_fields() {
            atom.score(haystack.slice(..), matcher)?;
        }
    }
    if prepared.positive.is_empty() {
        return Some((MatchTier::Extra, 0));
    }
    let mut tier = MatchTier::NameFuzzy;
    let mut raw = 0u32;
    for atom in &prepared.positive {
        let (field_tier, field_score) =
            entry
                .weighted_fields()
                .into_iter()
                .find_map(|(field_tier, haystack)| {
                    atom.score(haystack.slice(..), matcher)
                        .map(|score| (field_tier, score))
                })?;
        tier = tier.min(field_tier);
        raw += u32::from(field_score);
    }
    if tier == MatchTier::NameFuzzy && prepared.negative.is_empty() {
        if entry.name_key == prepared.text_key {
            return Some((MatchTier::NameExact, 0));
        }
        if entry.name_key.starts_with(&prepared.text_key) {
            return Some((
                MatchTier::NamePrefix,
                PREFIX_RAW.saturating_sub(entry.name_chars),
            ));
        }
    }
    Some((tier, raw))
}

/// Sorted, deduplicated char indices into the name for every positive atom
/// that matches the name (atoms matched elsewhere contribute nothing).
fn name_indices(entry: &Entry, prepared: &Prepared, matcher: &mut Matcher) -> Vec<u32> {
    let mut all = Vec::new();
    let mut atom_indices = Vec::new();
    for atom in &prepared.positive {
        atom_indices.clear();
        if atom
            .indices(entry.name.slice(..), matcher, &mut atom_indices)
            .is_some()
        {
            all.extend_from_slice(&atom_indices);
        }
    }
    all.sort_unstable();
    all.dedup();
    all
}

/// Case-insensitive, whitespace-collapsed comparison key.
fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn truncate_chars(value: &str, max_chars: usize) -> &str {
    match value.char_indices().nth(max_chars) {
        Some((byte, _)) => &value[..byte],
        None => value,
    }
}
