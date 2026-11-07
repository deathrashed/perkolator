//! The shared in-memory catalogue: scanned records plus the search index.
//! Scanning runs on the background executor; the UI only reads snapshots.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{Context, EventEmitter, Task};
use perkolator_core::{CancelToken, ResourceId, ResourceRecord};
use perkolator_registry::{Registry, ScanConfig, scan};
use perkolator_search::{Boosts, Query, SearchHit, SearchIndex};

pub enum LoadState {
    Loading,
    Ready { problems: usize },
    Failed(String),
}

pub enum CatalogEvent {
    Loaded,
}

struct Snapshot {
    records: Vec<Arc<ResourceRecord>>,
    by_id: HashMap<ResourceId, usize>,
    index: SearchIndex,
    problems: usize,
}

pub struct Catalog {
    snapshot: Arc<Snapshot>,
    state: LoadState,
    boosts: Boosts,
    _scan: Option<Task<()>>,
    _cancel: CancelToken,
}

impl EventEmitter<CatalogEvent> for Catalog {}

impl Catalog {
    pub fn load(home: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let cancel = CancelToken::new();
        let scan_cancel = cancel.clone();
        let task = cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let result = cx
                .background_executor()
                .spawn(async move { build_snapshot(home, &scan_cancel) })
                .await;
            let took = started.elapsed();
            let _ = this.update(cx, |catalog, cx| {
                match result {
                    Ok(snapshot) => {
                        if std::env::var_os("PERKOLATOR_DEBUG").is_some() {
                            eprintln!(
                                "perkolator: indexed {} resources in {:.1} ms ({} scan issues)",
                                snapshot.records.len(),
                                took.as_secs_f64() * 1000.0,
                                snapshot.problems
                            );
                        }
                        catalog.state = LoadState::Ready {
                            problems: snapshot.problems,
                        };
                        catalog.snapshot = Arc::new(snapshot);
                    }
                    Err(message) => catalog.state = LoadState::Failed(message),
                }
                cx.emit(CatalogEvent::Loaded);
                cx.notify();
            });
        });
        Self {
            snapshot: Arc::new(Snapshot {
                records: Vec::new(),
                by_id: HashMap::new(),
                index: SearchIndex::new(),
                problems: 0,
            }),
            state: LoadState::Loading,
            boosts: Boosts::default(),
            _scan: Some(task),
            _cancel: cancel,
        }
    }

    pub fn state(&self) -> &LoadState {
        &self.state
    }

    pub fn len(&self) -> usize {
        self.snapshot.records.len()
    }

    pub fn get(&self, id: &ResourceId) -> Option<Arc<ResourceRecord>> {
        self.snapshot
            .by_id
            .get(id)
            .map(|ix| self.snapshot.records[*ix].clone())
    }

    pub fn search(&self, query: &Query, limit: usize) -> Vec<SearchHit> {
        self.snapshot.index.search(query, &self.boosts, limit)
    }

    /// Record a use so the resource ranks higher (bounded boost).
    pub fn note_used(&mut self, id: &ResourceId) {
        let recent = &mut self.boosts.recent;
        for rank in recent.values_mut() {
            *rank = rank.saturating_add(1);
        }
        recent.insert(id.clone(), 0);
        const MAX_RECENT: u32 = 50;
        recent.retain(|_, rank| *rank < MAX_RECENT);
    }
}

fn build_snapshot(home: Option<PathBuf>, cancel: &CancelToken) -> Result<Snapshot, String> {
    let projects: Vec<PathBuf> = std::env::var_os("PERKOLATOR_PROJECTS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    let config = match home {
        Some(home) => ScanConfig::well_known(&home, &projects),
        None => ScanConfig::for_current_user(&projects)
            .ok_or_else(|| "cannot determine the home directory".to_owned())?,
    };
    let report = scan(&config, cancel);
    let problems = report.problems.len() + report.broken_links.len();
    let registry = Registry::from_report(&report);
    let records: Vec<Arc<ResourceRecord>> =
        registry.into_records().into_iter().map(Arc::new).collect();
    let index = SearchIndex::from_records(records.iter().map(|record| record.as_ref()));
    let by_id = records
        .iter()
        .enumerate()
        .map(|(ix, record)| (record.id.clone(), ix))
        .collect();
    Ok(Snapshot {
        records,
        by_id,
        index,
        problems,
    })
}
