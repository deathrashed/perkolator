//! Remote marketplace search for the launcher: debounced federated queries,
//! per-source status, and merged results. Network work runs on the
//! marketplace scheduler's threads; this entity only collects updates.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use perkolator_core::{ProviderId, ResourceKind};
use perkolator_marketplace::config::MarketplaceSettings;
use perkolator_marketplace::dedup::{MergedItem, merge};
use perkolator_marketplace::federated::{
    FederatedSearch, SkipReason, SourceOutcome, SourceSelection, SourceUpdate,
};
use perkolator_marketplace::{
    AuthStatus, ProviderDescriptor, ProviderEnv, RemoteItem, SearchRequest,
};

/// Remote lookups wait for typing to pause; local search never waits.
const REMOTE_DEBOUNCE: Duration = Duration::from_millis(300);
const MIN_QUERY_CHARS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceState {
    Idle,
    Pending,
    Done { count: usize, cached: bool },
    Skipped(SkipReason),
    Failed(String),
    TimedOut,
}

impl SourceState {
    pub fn label(&self) -> String {
        match self {
            SourceState::Idle => String::new(),
            SourceState::Pending => "Searching…".into(),
            SourceState::Done { count, cached } => {
                let cached = if *cached { " (cached)" } else { "" };
                format!("{count} results{cached}")
            }
            SourceState::Skipped(SkipReason::RemoteSearchOff) => "Remote search off".into(),
            SourceState::Skipped(SkipReason::Disabled) => "Disabled".into(),
            SourceState::Skipped(SkipReason::NotSelected) => "Not selected".into(),
            SourceState::Skipped(SkipReason::KindMismatch) => "No matching kinds".into(),
            SourceState::Skipped(SkipReason::Auth(status)) => status.label().into(),
            SourceState::Failed(message) => message.clone(),
            SourceState::TimedOut => "Timed out".into(),
        }
    }
}

pub struct SourceRow {
    pub descriptor: ProviderDescriptor,
    pub auth: AuthStatus,
    pub state: SourceState,
}

pub enum MarketplaceEvent {
    Updated,
}

pub struct Marketplace {
    search: Arc<FederatedSearch>,
    settings_path: Option<PathBuf>,
    states: BTreeMap<ProviderId, SourceState>,
    items: Vec<RemoteItem>,
    merged: Vec<MergedItem>,
    generation: u64,
    save_error: Option<String>,
    _debounce: Option<Task<()>>,
    _receiver: Option<Task<()>>,
}

impl EventEmitter<MarketplaceEvent> for Marketplace {}

impl Marketplace {
    pub fn new() -> Self {
        let settings_path = dirs::config_dir().map(|dir| dir.join("perkolator/marketplaces.toml"));
        let settings = settings_path
            .as_deref()
            .map(MarketplaceSettings::load)
            .transpose()
            .unwrap_or_else(|error| {
                eprintln!("perkolator: {error}; using marketplace defaults");
                None
            })
            .unwrap_or_else(MarketplaceSettings::defaults);
        let search = FederatedSearch::new(
            perkolator_providers::builtin(),
            ProviderEnv::system(),
            settings,
        );
        Self {
            search: Arc::new(search),
            settings_path,
            states: BTreeMap::new(),
            items: Vec::new(),
            merged: Vec::new(),
            generation: 0,
            save_error: None,
            _debounce: None,
            _receiver: None,
        }
    }

    pub fn settings(&self) -> MarketplaceSettings {
        self.search.settings()
    }

    pub fn merged(&self) -> &[MergedItem] {
        &self.merged
    }

    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    pub fn rows(&self) -> Vec<SourceRow> {
        self.search
            .providers()
            .iter()
            .map(|provider| {
                let descriptor = provider.descriptor().clone();
                SourceRow {
                    auth: provider.auth_status(self.search.env()),
                    state: self
                        .states
                        .get(&descriptor.id)
                        .cloned()
                        .unwrap_or(SourceState::Idle),
                    descriptor,
                }
            })
            .collect()
    }

    /// Short header summary, or `None` when remote search is off.
    pub fn summary(&self) -> Option<String> {
        let settings = self.search.settings();
        if !settings.remote_search_enabled {
            return None;
        }
        let pending = self
            .states
            .values()
            .filter(|state| **state == SourceState::Pending)
            .count();
        let enabled = self
            .search
            .providers()
            .iter()
            .filter(|provider| settings.is_enabled(&provider.descriptor().id))
            .count();
        Some(if pending > 0 {
            format!("{pending} of {enabled} sources searching")
        } else {
            format!("{enabled} sources")
        })
    }

    /// Apply edited settings (from the source popover) and persist them.
    pub fn apply_settings(&mut self, settings: MarketplaceSettings, cx: &mut Context<Self>) {
        self.search
            .update_settings(|current| *current = settings.clone());
        self.save_error = self
            .settings_path
            .as_deref()
            .and_then(|path| settings.save(path).err())
            .map(|error| error.to_string());
        self.search.clear_cache();
        cx.emit(MarketplaceEvent::Updated);
        cx.notify();
    }

    /// Debounced federated search. Clears results for short queries.
    pub fn query(&mut self, text: &str, kinds: BTreeSet<ResourceKind>, cx: &mut Context<Self>) {
        self.items.clear();
        self.merged.clear();
        self.states.clear();
        self._receiver = None;
        let text = text.trim().to_owned();
        if text.chars().count() < MIN_QUERY_CHARS || !self.search.settings().remote_search_enabled {
            self._debounce = None;
            cx.emit(MarketplaceEvent::Updated);
            cx.notify();
            return;
        }
        self._debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REMOTE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| this.start(text, kinds, cx));
        }));
        cx.emit(MarketplaceEvent::Updated);
        cx.notify();
    }

    fn start(&mut self, text: String, kinds: BTreeSet<ResourceKind>, cx: &mut Context<Self>) {
        let mut request = SearchRequest::new(text);
        request.kinds = kinds;
        let (sender, receiver) = async_channel::unbounded::<SourceUpdate>();
        let handle = self.search.start(
            request,
            &SourceSelection::AllEnabled,
            Arc::new(move |update| {
                let _ = sender.send_blocking(update);
            }),
        );
        self.generation = handle.generation;
        for source in &handle.scheduled {
            self.states.insert(source.clone(), SourceState::Pending);
        }
        self._receiver = Some(cx.spawn(async move |this, cx| {
            while let Ok(update) = receiver.recv().await {
                if this.update(cx, |this, cx| this.apply(update, cx)).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn apply(&mut self, update: SourceUpdate, cx: &mut Context<Self>) {
        if update.generation != self.generation {
            return;
        }
        let state = match update.outcome {
            SourceOutcome::Skipped(reason) => SourceState::Skipped(reason),
            SourceOutcome::Results {
                page, from_cache, ..
            } => {
                let count = page.items.len();
                self.items.extend(page.items);
                self.merged = merge(self.items.iter().cloned());
                SourceState::Done {
                    count,
                    cached: from_cache,
                }
            }
            SourceOutcome::Failed { error, .. } => SourceState::Failed(error.to_string()),
            SourceOutcome::TimedOut => SourceState::TimedOut,
        };
        self.states.insert(update.source, state);
        cx.emit(MarketplaceEvent::Updated);
        cx.notify();
    }
}
