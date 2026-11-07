//! Concurrent, cancellable search across enabled sources.
//!
//! Guarantees (tests/MARKETPLACE_ACCEPTANCE.md): disabled sources receive no
//! request; kind and auth filters apply before scheduling; each source has
//! its own timeout; results stream per source as they settle; a newer
//! search supersedes older ones and stale updates are never delivered.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use perkolator_core::{CancelToken, ProviderId};

use crate::config::{MarketplaceSettings, SearchMode};
use crate::{AuthStatus, MarketplaceProvider, ProviderEnv, ProviderError, SearchPage, SearchRequest};

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// Which sources the user asked for in this search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SourceSelection {
    /// Every source enabled in settings.
    #[default]
    AllEnabled,
    /// Only these (still subject to being enabled).
    Only(BTreeSet<ProviderId>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    NotSelected,
    KindMismatch,
    Auth(AuthStatus),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceOutcome {
    Skipped(SkipReason),
    Results {
        page: SearchPage,
        from_cache: bool,
        elapsed: Duration,
    },
    Failed {
        error: ProviderError,
        elapsed: Duration,
    },
    TimedOut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceUpdate {
    pub generation: u64,
    pub source: ProviderId,
    pub outcome: SourceOutcome,
}

/// Returned to the caller; dropping it does not cancel (call `cancel`).
#[derive(Clone, Debug)]
pub struct SearchHandle {
    pub generation: u64,
    pub scheduled: Vec<ProviderId>,
    cancel: CancelToken,
}

impl SearchHandle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

type CacheKey = (ProviderId, String);

struct CacheEntry {
    page: SearchPage,
    stored: Instant,
}

pub struct FederatedSearch {
    providers: Vec<Arc<dyn MarketplaceProvider>>,
    env: ProviderEnv,
    settings: Arc<Mutex<MarketplaceSettings>>,
    cache: Arc<Mutex<HashMap<CacheKey, CacheEntry>>>,
    generation: Arc<AtomicU64>,
    current_cancel: Mutex<Option<CancelToken>>,
}

impl FederatedSearch {
    pub fn new(
        providers: Vec<Arc<dyn MarketplaceProvider>>,
        env: ProviderEnv,
        settings: MarketplaceSettings,
    ) -> Self {
        Self {
            providers,
            env,
            settings: Arc::new(Mutex::new(settings)),
            cache: Arc::new(Mutex::new(HashMap::new())),
            generation: Arc::new(AtomicU64::new(0)),
            current_cancel: Mutex::new(None),
        }
    }

    pub fn providers(&self) -> &[Arc<dyn MarketplaceProvider>] {
        &self.providers
    }

    pub fn env(&self) -> &ProviderEnv {
        &self.env
    }

    pub fn settings(&self) -> MarketplaceSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn update_settings(&self, edit: impl FnOnce(&mut MarketplaceSettings)) {
        edit(&mut self.settings.lock().unwrap());
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    /// Decide what each source would do for `request`, without any I/O
    /// beyond cheap local auth checks.
    pub fn plan(
        &self,
        request: &SearchRequest,
        selection: &SourceSelection,
    ) -> Vec<(Arc<dyn MarketplaceProvider>, Option<SkipReason>)> {
        let settings = self.settings();
        self.providers
            .iter()
            .map(|provider| {
                let descriptor = provider.descriptor();
                let skip = if !settings.is_enabled(&descriptor.id) {
                    Some(SkipReason::Disabled)
                } else if matches!(selection, SourceSelection::Only(ids) if !ids.contains(&descriptor.id))
                    || (settings.mode == SearchMode::SelectedOnly
                        && matches!(selection, SourceSelection::AllEnabled))
                {
                    Some(SkipReason::NotSelected)
                } else if !descriptor.kinds.iter().any(|kind| request.wants(*kind))
                    || !descriptor.capabilities.search
                {
                    Some(SkipReason::KindMismatch)
                } else {
                    let auth = provider.auth_status(&self.env);
                    (!auth.allows_search()).then_some(SkipReason::Auth(auth))
                };
                (provider.clone(), skip)
            })
            .collect()
    }

    /// Start a search; `sink` receives one update per source, from worker
    /// threads, only while this generation is current.
    pub fn start(
        &self,
        request: SearchRequest,
        selection: &SourceSelection,
        sink: Arc<dyn Fn(SourceUpdate) + Send + Sync>,
    ) -> SearchHandle {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let cancel = CancelToken::new();
        if let Some(previous) = self.current_cancel.lock().unwrap().replace(cancel.clone()) {
            previous.cancel();
        }
        let settings = self.settings();
        let mut queue = VecDeque::new();
        let mut scheduled = Vec::new();
        for (provider, skip) in self.plan(&request, selection) {
            let source = provider.descriptor().id.clone();
            match skip {
                Some(reason) => sink(SourceUpdate {
                    generation,
                    source,
                    outcome: SourceOutcome::Skipped(reason),
                }),
                None => {
                    let key = (source.clone(), cache_key(&request));
                    let cached = self
                        .cache
                        .lock()
                        .unwrap()
                        .get(&key)
                        .filter(|entry| entry.stored.elapsed() < CACHE_TTL)
                        .map(|entry| entry.page.clone());
                    if let Some(page) = cached {
                        sink(SourceUpdate {
                            generation,
                            source,
                            outcome: SourceOutcome::Results {
                                page,
                                from_cache: true,
                                elapsed: Duration::ZERO,
                            },
                        });
                    } else {
                        scheduled.push(source);
                        queue.push_back(provider);
                    }
                }
            }
        }

        let queue = Arc::new(Mutex::new(queue));
        let workers = settings.max_concurrent_sources.min(queue.lock().unwrap().len());
        for _ in 0..workers {
            let queue = queue.clone();
            let env = self.env.clone();
            let request = request.clone();
            let cancel = cancel.clone();
            let sink = sink.clone();
            let cache = self.cache.clone();
            let current = self.generation.clone();
            let timeout = settings.remote_timeout;
            std::thread::Builder::new()
                .name("perkolator-marketplace".into())
                .spawn(move || {
                    loop {
                        let Some(provider) = queue.lock().unwrap().pop_front() else {
                            break;
                        };
                        if cancel.is_cancelled() {
                            break;
                        }
                        let source = provider.descriptor().id.clone();
                        let outcome = run_with_timeout(&provider, &request, &env, &cancel, timeout);
                        if let SourceOutcome::Results { page, .. } = &outcome {
                            cache.lock().unwrap().insert(
                                (source.clone(), cache_key(&request)),
                                CacheEntry {
                                    page: page.clone(),
                                    stored: Instant::now(),
                                },
                            );
                        }
                        if current.load(Ordering::Acquire) == generation && !cancel.is_cancelled() {
                            sink(SourceUpdate {
                                generation,
                                source,
                                outcome,
                            });
                        }
                    }
                })
                .expect("spawn marketplace worker");
        }

        SearchHandle {
            generation,
            scheduled,
            cancel,
        }
    }

    /// Drop cached pages (explicit refresh).
    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }
}

fn cache_key(request: &SearchRequest) -> String {
    let kinds: Vec<&str> = request.kinds.iter().map(|kind| kind.slug()).collect();
    format!(
        "v1|{}|{}|{}|{}|{}",
        request.query.trim().to_lowercase(),
        kinds.join(","),
        request.category.as_deref().unwrap_or(""),
        request.page_size,
        request.cursor.as_deref().unwrap_or("")
    )
}

fn run_with_timeout(
    provider: &Arc<dyn MarketplaceProvider>,
    request: &SearchRequest,
    env: &ProviderEnv,
    cancel: &CancelToken,
    timeout: Duration,
) -> SourceOutcome {
    let started = Instant::now();
    let (sender, receiver) = mpsc::channel();
    let provider = provider.clone();
    let request = request.clone();
    let env = env.clone();
    let call_cancel = cancel.clone();
    std::thread::spawn(move || {
        let _ = sender.send(provider.search(&request, &env, &call_cancel));
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(page)) => SourceOutcome::Results {
            page,
            from_cache: false,
            elapsed: started.elapsed(),
        },
        Ok(Err(error)) => SourceOutcome::Failed {
            error,
            elapsed: started.elapsed(),
        },
        Err(_) => SourceOutcome::TimedOut,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use perkolator_core::ResourceKind;

    use super::*;
    use crate::testing::offline_env;
    use crate::{
        AuthStrategy, ProviderCapabilities, ProviderDescriptor, RemoteActions, RemoteItem,
        Transport,
    };

    struct Fake {
        descriptor: ProviderDescriptor,
        calls: AtomicUsize,
        delay: Duration,
        result: Result<usize, ProviderError>,
        auth: AuthStatus,
    }

    impl Fake {
        fn new(id: &str, kinds: &[ResourceKind]) -> Self {
            Self {
                descriptor: ProviderDescriptor {
                    id: ProviderId::new(id).unwrap(),
                    display_name: id.into(),
                    homepage: "https://example.invalid".into(),
                    docs_url: None,
                    kinds: kinds.to_vec(),
                    transport: Transport::Rest,
                    auth: AuthStrategy::None,
                    capabilities: ProviderCapabilities {
                        search: true,
                        ..Default::default()
                    },
                    notes: vec![],
                },
                calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
                result: Ok(1),
                auth: AuthStatus::NotRequired,
            }
        }
    }

    impl MarketplaceProvider for Fake {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        fn auth_status(&self, _env: &ProviderEnv) -> AuthStatus {
            self.auth.clone()
        }

        fn search(
            &self,
            request: &SearchRequest,
            _env: &ProviderEnv,
            _cancel: &CancelToken,
        ) -> Result<SearchPage, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            let count = self.result.clone()?;
            Ok(SearchPage {
                items: (0..count)
                    .map(|ix| RemoteItem {
                        source: self.descriptor.id.clone(),
                        upstream_id: format!("{}-{ix}", request.query),
                        kind: self.descriptor.kinds[0],
                        title: request.query.clone(),
                        summary: String::new(),
                        canonical_url: None,
                        repo_url: None,
                        resource_path: None,
                        package_ref: None,
                        version: None,
                        author: None,
                        categories: vec![],
                        actions: RemoteActions::default(),
                        fetched_at_unix: 0,
                    })
                    .collect(),
                next_cursor: None,
                total: None,
            })
        }
    }

    fn settings(enabled: &[&str]) -> MarketplaceSettings {
        let mut settings = MarketplaceSettings::from_toml("[marketplaces]\nremote_timeout_ms = 500").unwrap();
        for id in enabled {
            settings.set_enabled(ProviderId::new(*id).unwrap(), true);
        }
        settings
    }

    fn collect(search: &FederatedSearch, request: SearchRequest, selection: SourceSelection) -> Vec<SourceUpdate> {
        let updates = Arc::new(Mutex::new(Vec::new()));
        let sink_updates = updates.clone();
        let handle = search.start(
            request,
            &selection,
            Arc::new(move |update| sink_updates.lock().unwrap().push(update)),
        );
        let expected = search.providers().len();
        let deadline = Instant::now() + Duration::from_secs(5);
        while updates.lock().unwrap().len() < expected && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = handle;
        updates.lock().unwrap().clone()
    }

    fn outcome_for<'a>(updates: &'a [SourceUpdate], id: &str) -> &'a SourceOutcome {
        &updates.iter().find(|update| update.source.as_str() == id).unwrap().outcome
    }

    #[test]
    fn disabled_and_mismatched_sources_get_no_calls() {
        let enabled = Arc::new(Fake::new("on", &[ResourceKind::Skill]));
        let disabled = Arc::new(Fake::new("off", &[ResourceKind::Skill]));
        let mcp_only = Arc::new(Fake::new("mcp", &[ResourceKind::McpServer]));
        let (env, _, _) = offline_env();
        let search = FederatedSearch::new(
            vec![enabled.clone(), disabled.clone(), mcp_only.clone()],
            env,
            settings(&["on", "mcp"]),
        );
        let mut request = SearchRequest::new("pdf");
        request.kinds.insert(ResourceKind::Skill);
        let updates = collect(&search, request, SourceSelection::AllEnabled);
        assert_eq!(disabled.calls.load(Ordering::SeqCst), 0);
        assert_eq!(mcp_only.calls.load(Ordering::SeqCst), 0);
        assert_eq!(enabled.calls.load(Ordering::SeqCst), 1);
        assert_eq!(outcome_for(&updates, "off"), &SourceOutcome::Skipped(SkipReason::Disabled));
        assert_eq!(outcome_for(&updates, "mcp"), &SourceOutcome::Skipped(SkipReason::KindMismatch));
        assert!(matches!(outcome_for(&updates, "on"), SourceOutcome::Results { .. }));
    }

    #[test]
    fn one_failure_or_timeout_does_not_block_others() {
        let mut slow = Fake::new("slow", &[ResourceKind::Skill]);
        slow.delay = Duration::from_secs(2);
        let mut broken = Fake::new("broken", &[ResourceKind::Skill]);
        broken.result = Err(ProviderError::Forbidden);
        let ok = Fake::new("ok", &[ResourceKind::Skill]);
        let (env, _, _) = offline_env();
        let search = FederatedSearch::new(
            vec![Arc::new(slow), Arc::new(broken), Arc::new(ok)],
            env,
            settings(&["slow", "broken", "ok"]),
        );
        let updates = collect(&search, SearchRequest::new("x"), SourceSelection::AllEnabled);
        assert_eq!(outcome_for(&updates, "slow"), &SourceOutcome::TimedOut);
        assert!(matches!(
            outcome_for(&updates, "broken"),
            SourceOutcome::Failed { error: ProviderError::Forbidden, .. }
        ));
        assert!(matches!(outcome_for(&updates, "ok"), SourceOutcome::Results { .. }));
    }

    #[test]
    fn auth_gate_and_selection_are_applied_before_scheduling() {
        let mut locked = Fake::new("locked", &[ResourceKind::McpServer]);
        locked.auth = AuthStatus::NeedsAccess;
        let locked = Arc::new(locked);
        let other = Arc::new(Fake::new("other", &[ResourceKind::McpServer]));
        let chosen = Arc::new(Fake::new("chosen", &[ResourceKind::McpServer]));
        let (env, _, _) = offline_env();
        let search = FederatedSearch::new(
            vec![locked.clone(), other.clone(), chosen.clone()],
            env,
            settings(&["locked", "other", "chosen"]),
        );
        let only = SourceSelection::Only(
            ["locked", "chosen"].into_iter().map(|id| ProviderId::new(id).unwrap()).collect(),
        );
        let updates = collect(&search, SearchRequest::new("db"), only);
        assert_eq!(locked.calls.load(Ordering::SeqCst), 0);
        assert_eq!(other.calls.load(Ordering::SeqCst), 0);
        assert_eq!(chosen.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcome_for(&updates, "locked"),
            &SourceOutcome::Skipped(SkipReason::Auth(AuthStatus::NeedsAccess))
        );
        assert_eq!(outcome_for(&updates, "other"), &SourceOutcome::Skipped(SkipReason::NotSelected));
    }

    #[test]
    fn newer_search_suppresses_stale_updates_and_cache_avoids_repeat_calls() {
        let mut slow = Fake::new("slow", &[ResourceKind::Prompt]);
        slow.delay = Duration::from_millis(200);
        let slow = Arc::new(slow);
        let (env, _, _) = offline_env();
        let search = FederatedSearch::new(vec![slow.clone()], env, settings(&["slow"]));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let sink_updates = updates.clone();
        let sink: Arc<dyn Fn(SourceUpdate) + Send + Sync> =
            Arc::new(move |update| sink_updates.lock().unwrap().push(update));
        let first = search.start(SearchRequest::new("a"), &SourceSelection::AllEnabled, sink.clone());
        let second = search.start(SearchRequest::new("ab"), &SourceSelection::AllEnabled, sink.clone());
        std::thread::sleep(Duration::from_millis(600));
        let seen: Vec<u64> = updates.lock().unwrap().iter().map(|update| update.generation).collect();
        assert!(!seen.contains(&first.generation), "stale generation delivered: {seen:?}");
        assert!(seen.contains(&second.generation));
        assert!(search.is_current(second.generation));

        let calls_before = slow.calls.load(Ordering::SeqCst);
        let third = search.start(SearchRequest::new("ab"), &SourceSelection::AllEnabled, sink);
        assert!(third.scheduled.is_empty(), "cached query must not be rescheduled");
        assert_eq!(slow.calls.load(Ordering::SeqCst), calls_before);
    }
}
