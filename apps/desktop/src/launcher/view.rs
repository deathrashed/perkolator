use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, HighlightStyle, InteractiveElement as _, IntoElement,
    Keystroke, ParentElement as _, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Subscription, Task,
    UniformListScrollHandle, Window, div, img, prelude::FluentBuilder as _, uniform_list,
};
use perkolator_actions::{
    ActionContext, ActionInputs, ComposeFormat, Surface, actions_for, compose_effect,
    compose_payload, default_action, deliver, resolve,
};
use perkolator_core::{
    ActionAvailability, ActionEffect, ActionId, ActionOutcome, FeatureSupport, InstallState,
    IssueSeverity, LocationRole, PlatformFeature, RelPath, ResourceKind, ResourceRecord,
    SourceKind,
};
use perkolator_preview::{
    Document, PreviewLimits, PreviewScheduler, PreviewTab, build_preview, preview_file,
};
use perkolator_search::{Query, ScopeFilter, SearchHit, reselect};

use perkolator_marketplace::dedup::MergedItem;

use super::model::{
    ActionMenu, BackStep, Layout, MenuAction, MenuEntry, Mode, RemoteAction, RowRef, back_step,
    build_rows, fence, highlight_ranges, human_size, launcher_supports, remote_entries, step_row,
    step_selection,
};
use super::remote::{self, SourceMenu};
use super::{
    ACTIONS_KEY, Back, CONTEXT, Confirm, EnterCompose, LauncherMemory, NextFile, NextTab,
    OpenActions, OpenSources, PrevFile, PrevTab, SOURCES_KEY, SelectNext, SelectPageNext,
    SelectPagePrev, SelectPrev, ToggleSource, UseCompactLayout, UseExpandedLayout, UseSplitLayout,
};
use crate::catalog::{Catalog, CatalogEvent, LoadState};
use crate::marketplace::{Marketplace, MarketplaceEvent};
use crate::platform;
use crate::theme_bridge::Palette;

const RESULT_LIMIT: usize = 500;
const PAGE_ROWS: isize = 10;
const STATUS_VISIBLE: Duration = Duration::from_secs(5);

pub enum LauncherEvent {
    Dismiss,
    Relayout(Layout),
}

enum PreviewSlot {
    Idle,
    Loading,
    Ready(Arc<perkolator_preview::Preview>),
    Failed(SharedString),
}

struct FileView {
    path: RelPath,
    document: Option<Document>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Done,
    Fallback,
    Error,
}

struct Status {
    text: SharedString,
    tone: Tone,
}

pub struct LauncherView {
    focus: FocusHandle,
    catalog: Entity<Catalog>,
    marketplace: Entity<Marketplace>,
    sources: Option<SourceMenu>,
    query: Entity<InputState>,
    compose: Entity<InputState>,
    mode: Mode,
    layout: Layout,
    parsed: Query,
    hits: Vec<SearchHit>,
    selected: usize,
    scroll: UniformListScrollHandle,
    scheduler: PreviewScheduler,
    limits: PreviewLimits,
    preview: PreviewSlot,
    preview_tab: usize,
    file_ix: Option<usize>,
    file_view: Option<FileView>,
    menu: Option<ActionMenu>,
    status: Option<Status>,
    features: BTreeMap<PlatformFeature, FeatureSupport>,
    last_search: Duration,
    preview_task: Option<Task<()>>,
    file_task: Option<Task<()>>,
    status_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LauncherEvent> for LauncherView {}

impl Focusable for LauncherView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LauncherView {
    pub fn new(
        catalog: Entity<Catalog>,
        marketplace: Entity<Marketplace>,
        memory: LauncherMemory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search skills, MCP servers, agents, prompts, rules, plugins")
                .default_value(memory.query.clone())
        });
        let compose = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Ask the agent… (Enter copies the composed text)")
        });
        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_results(cx);
                    this.query_marketplace(cx);
                }
            }),
            cx.subscribe(
                &marketplace,
                |this, _, event: &MarketplaceEvent, cx| match event {
                    MarketplaceEvent::Updated => this.on_remote_updated(cx),
                },
            ),
            cx.subscribe_in(&compose, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&catalog, |this, _, event: &CatalogEvent, cx| match event {
                CatalogEvent::Loaded => this.refresh_results(cx),
            }),
        ];
        let mut view = Self {
            focus: cx.focus_handle(),
            catalog,
            marketplace,
            sources: None,
            query,
            compose,
            mode: Mode::Search,
            layout: memory.layout,
            parsed: Query::parse(&memory.query),
            hits: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            scheduler: PreviewScheduler::new(),
            limits: PreviewLimits::default(),
            preview: PreviewSlot::Idle,
            preview_tab: 0,
            file_ix: None,
            file_view: None,
            menu: None,
            status: None,
            features: platform::features(),
            last_search: Duration::ZERO,
            preview_task: None,
            file_task: None,
            status_task: None,
            _subscriptions: subscriptions,
        };
        view.run_search(memory.selected.as_ref(), cx);
        view.query_marketplace(cx);
        view.query.update(cx, |input, cx| input.focus(window, cx));
        view
    }

    pub fn memory(&self, cx: &App) -> LauncherMemory {
        LauncherMemory {
            query: self.query.read(cx).value().to_string(),
            selected: self.selected_local_hit(cx).map(|hit| hit.id.clone()),
            layout: self.layout,
        }
    }

    // ---- search & selection -------------------------------------------------

    fn rows(&self, cx: &App) -> Vec<RowRef> {
        build_rows(self.hits.len(), self.marketplace.read(cx).merged().len())
    }

    fn selected_row(&self, cx: &App) -> Option<RowRef> {
        self.rows(cx).get(self.selected).copied()
    }

    fn selected_local_hit(&self, cx: &App) -> Option<&SearchHit> {
        match self.selected_row(cx)? {
            RowRef::Local(ix) => self.hits.get(ix),
            _ => None,
        }
    }

    fn selected_remote(&self, cx: &App) -> Option<MergedItem> {
        match self.selected_row(cx)? {
            RowRef::Remote(ix) => self.marketplace.read(cx).merged().get(ix).cloned(),
            _ => None,
        }
    }

    fn refresh_results(&mut self, cx: &mut Context<Self>) {
        let previous = self.selected_local_hit(cx).map(|hit| hit.id.clone());
        self.run_search(previous.as_ref(), cx);
        cx.notify();
    }

    fn query_marketplace(&mut self, cx: &mut Context<Self>) {
        let text = self.parsed.text.clone();
        let kinds = self.parsed.kinds.clone();
        self.marketplace
            .update(cx, |marketplace, cx| marketplace.query(&text, kinds, cx));
    }

    /// Remote results arrived: keep the local selection; clamp otherwise.
    fn on_remote_updated(&mut self, cx: &mut Context<Self>) {
        let len = self.rows(cx).len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
        if let Some(menu) = &mut self.sources {
            let rows = self.marketplace.read(cx).rows();
            let selected = menu.selected;
            let draft = menu.draft.clone();
            *menu = SourceMenu::new(draft, &rows);
            menu.selected = selected.min(menu.rows.len().saturating_sub(1));
        }
        cx.notify();
    }

    fn run_search(&mut self, keep: Option<&perkolator_core::ResourceId>, cx: &mut Context<Self>) {
        let text = self.query.read(cx).value().to_string();
        self.parsed = Query::parse(&text);
        let started = Instant::now();
        self.hits = self.catalog.read(cx).search(&self.parsed, RESULT_LIMIT);
        self.last_search = started.elapsed();
        if std::env::var_os("PERKOLATOR_DEBUG").is_some() {
            eprintln!(
                "perkolator: search {:?} → {} hits in {:.2} ms",
                text,
                self.hits.len(),
                self.last_search.as_secs_f64() * 1000.0
            );
        }
        let ix = reselect(keep, &self.hits);
        self.select(ix, cx);
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        self.selected = ix;
        if !rows.is_empty() {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        }
        match rows.get(ix) {
            Some(RowRef::Local(hit_ix)) => {
                let changed =
                    self.hits.get(*hit_ix).map(|hit| &hit.id) != self.current_preview_id().as_ref();
                if changed {
                    self.load_preview(cx);
                }
            }
            _ => {
                self.scheduler.cancel();
                self.preview = PreviewSlot::Idle;
                self.preview_task = None;
                self.file_view = None;
                self.file_ix = None;
            }
        }
        cx.notify();
    }

    fn current_preview_id(&self) -> Option<perkolator_core::ResourceId> {
        match &self.preview {
            PreviewSlot::Ready(preview) => Some(preview.id.clone()),
            _ => None,
        }
    }

    fn selected_record(&self, cx: &App) -> Option<Arc<ResourceRecord>> {
        let hit = self.selected_local_hit(cx)?;
        self.catalog.read(cx).get(&hit.id)
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Actions => {
                if let Some(menu) = &mut self.menu {
                    menu.move_by(delta.signum());
                    cx.notify();
                }
            }
            Mode::Sources => {
                if let Some(menu) = &mut self.sources {
                    menu.move_by(delta.signum());
                    cx.notify();
                }
            }
            Mode::Search => {
                let rows = self.rows(cx);
                let ix = step_row(&rows, self.selected, delta);
                if ix != self.selected {
                    self.select(ix, cx);
                }
            }
            Mode::Compose => {}
        }
    }

    // ---- preview ------------------------------------------------------------

    fn load_preview(&mut self, cx: &mut Context<Self>) {
        self.file_ix = None;
        self.file_view = None;
        self.file_task = None;
        let Some(record) = self.selected_record(cx) else {
            self.scheduler.cancel();
            self.preview = PreviewSlot::Idle;
            self.preview_task = None;
            return;
        };
        let (generation, cancel) = self.scheduler.begin();
        let limits = self.limits.clone();
        self.preview = PreviewSlot::Loading;
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { build_preview(&record, generation, &limits, &cancel) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.scheduler.is_current(generation) {
                    return;
                }
                this.preview = match result {
                    Ok(preview) => PreviewSlot::Ready(Arc::new(preview)),
                    Err(error) => PreviewSlot::Failed(error.to_string().into()),
                };
                this.preview_tab = 0;
                cx.notify();
            });
        }));
    }

    fn tabs(&self) -> &[PreviewTab] {
        match &self.preview {
            PreviewSlot::Ready(preview) => &preview.tabs,
            _ => &[],
        }
    }

    fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.tabs().len();
        if count == 0 {
            return;
        }
        self.preview_tab = (self.preview_tab as isize + delta).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    fn files_tab(&self) -> Option<(usize, &perkolator_preview::FileTree)> {
        self.tabs()
            .iter()
            .enumerate()
            .find_map(|(ix, tab)| match tab {
                PreviewTab::Files(tree) => Some((ix, tree)),
                _ => None,
            })
    }

    fn step_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some((tab_ix, tree)) = self.files_tab() else {
            return;
        };
        let files: Vec<usize> = tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.is_dir)
            .map(|(ix, _)| ix)
            .collect();
        if files.is_empty() {
            return;
        }
        let position = self
            .file_ix
            .and_then(|current| files.iter().position(|ix| *ix == current));
        let next = match position {
            None if delta >= 0 => 0,
            None => files.len() - 1,
            Some(position) => step_selection(position, files.len(), delta),
        };
        let node_ix = files[next];
        let path = tree.nodes[node_ix].path.clone();
        self.preview_tab = tab_ix;
        self.file_ix = Some(node_ix);
        self.open_file(path, cx);
    }

    fn open_file(&mut self, path: RelPath, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record(cx) else {
            return;
        };
        let Some(root) = record.details.browse_root().cloned() else {
            return;
        };
        let limits = self.limits.clone();
        let requested = path.clone();
        self.file_view = Some(FileView {
            path,
            document: None,
        });
        self.file_task = Some(cx.spawn(async move |this, cx| {
            let request = requested.clone();
            let document = cx
                .background_executor()
                .spawn(async move { preview_file(&root, &request, &limits) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(view) = &mut this.file_view
                    && view.path == requested
                {
                    view.document = Some(document);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    // ---- actions ------------------------------------------------------------

    fn action_context(&self) -> ActionContext {
        self.features.iter().fold(
            ActionContext::new(Surface::Launcher),
            |ctx, (feature, support)| ctx.with_feature(*feature, support.clone()),
        )
    }

    fn launcher_default(&self, record: &ResourceRecord) -> Option<ActionId> {
        let preferred = default_action(record);
        if launcher_supports(preferred) {
            return Some(preferred);
        }
        actions_for(record, &self.action_context())
            .into_iter()
            .find(|entry| entry.is_available() && launcher_supports(entry.id))
            .map(|entry| entry.id)
    }

    fn open_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries: Vec<MenuEntry> = if let Some(record) = self.selected_record(cx) {
            actions_for(&record, &self.action_context())
                .into_iter()
                .filter(|entry| launcher_supports(entry.id))
                .map(MenuEntry::from)
                .collect()
        } else if let Some(item) = self.selected_remote(cx) {
            remote_entries(
                remote_url(&item).is_some(),
                item.primary.package_ref.is_some(),
            )
        } else {
            return;
        };
        self.menu = Some(ActionMenu::new(entries));
        self.mode = Mode::Actions;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn run_remote_action(&mut self, action: RemoteAction, cx: &mut Context<Self>) {
        let Some(item) = self.selected_remote(cx) else {
            return;
        };
        match action {
            RemoteAction::CopySourceUrl => match remote_url(&item) {
                Some(url) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                    self.set_status("Copied source URL", Tone::Done, cx);
                }
                None => self.set_status("The source did not provide a URL", Tone::Error, cx),
            },
            RemoteAction::OpenInBrowser => match remote_url(&item) {
                Some(url) => {
                    cx.open_url(&url);
                    self.set_status(format!("Opened {url}"), Tone::Done, cx);
                }
                None => self.set_status("The source did not provide a URL", Tone::Error, cx),
            },
            RemoteAction::CopyPackageRef => {
                if let Some(package) = item.primary.package_ref.clone() {
                    cx.write_to_clipboard(ClipboardItem::new_string(package));
                    self.set_status("Copied package reference", Tone::Done, cx);
                }
            }
        }
    }

    fn open_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let marketplace = self.marketplace.read(cx);
        self.sources = Some(SourceMenu::new(marketplace.settings(), &marketplace.rows()));
        self.menu = None;
        self.mode = Mode::Sources;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn apply_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.sources.take() {
            self.marketplace.update(cx, |marketplace, cx| {
                marketplace.apply_settings(menu.draft, cx)
            });
            let saved = self.marketplace.read(cx).save_error().map(str::to_owned);
            match saved {
                Some(error) => {
                    self.set_status(format!("Settings not saved: {error}"), Tone::Error, cx)
                }
                None => self.set_status("Source settings saved", Tone::Done, cx),
            }
        }
        self.close_menu(window, cx);
        self.query_marketplace(cx);
    }

    fn on_open_sources(&mut self, _: &OpenSources, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Sources {
            self.sources = None;
            self.close_menu(window, cx);
        } else {
            self.open_sources(window, cx);
        }
    }

    fn on_toggle_source(&mut self, _: &ToggleSource, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.sources {
            menu.toggle_selected();
            cx.notify();
        }
    }

    fn run_action(&mut self, id: ActionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record(cx) else {
            return;
        };
        let inputs = ActionInputs::default();
        match resolve(id, &record, &self.action_context(), &inputs) {
            Ok(effect) => self.perform(effect, &record, window, cx),
            Err(error) => self.set_status(error.to_string(), Tone::Error, cx),
        }
    }

    fn perform(
        &mut self,
        effect: ActionEffect,
        record: &ResourceRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match effect {
            ActionEffect::WriteClipboard(payload) => {
                cx.write_to_clipboard(ClipboardItem::new_string(payload.text));
                let outcome = ActionOutcome::Copied {
                    label: payload.label,
                };
                self.note_used(record, cx);
                self.set_status(outcome.message(), Tone::Done, cx);
            }
            ActionEffect::Reveal { path } => {
                cx.reveal_path(&path);
                self.set_status(ActionOutcome::Revealed { path }.message(), Tone::Done, cx);
            }
            ActionEffect::OpenUrl { url } => {
                cx.open_url(&url);
                self.set_status(format!("Opened {url}"), Tone::Done, cx);
            }
            ActionEffect::EnterCompose { .. } => self.enter_compose(window, cx),
            ActionEffect::OpenDetails { .. } | ActionEffect::OpenDeployPlanner { .. } => {
                self.set_status(
                    "Deployment planning runs in the CLI for now: `perk deploy --dry-run`",
                    Tone::Fallback,
                    cx,
                );
            }
        }
    }

    fn note_used(&mut self, record: &ResourceRecord, cx: &mut Context<Self>) {
        let id = record.id.clone();
        self.catalog.update(cx, |catalog, _| catalog.note_used(&id));
    }

    fn set_status(&mut self, text: impl Into<SharedString>, tone: Tone, cx: &mut Context<Self>) {
        self.status = Some(Status {
            text: text.into(),
            tone,
        });
        self.status_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STATUS_VISIBLE).await;
            let _ = this.update(cx, |this, cx| {
                this.status = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn enter_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record(cx) else {
            return;
        };
        if !record
            .capabilities
            .contains(perkolator_core::Capability::Compose)
        {
            self.set_status(
                format!(
                    "{} resources cannot be composed into a query",
                    record.kind().label()
                ),
                Tone::Error,
                cx,
            );
            return;
        }
        self.mode = Mode::Compose;
        self.menu = None;
        self.compose.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn composed_text(&self, record: &ResourceRecord, cx: &App) -> String {
        let query = self.compose.read(cx).value().to_string();
        compose_payload(&[record], &query, ComposeFormat::Generic)
    }

    fn send_compose(&mut self, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record(cx) else {
            return;
        };
        let query = self.compose.read(cx).value().to_string();
        let effect = compose_effect(&[&record], &query, ComposeFormat::Generic);
        let ctx = self.action_context();
        let Some(delivery) = deliver(&effect, &ctx, true) else {
            return;
        };
        let payload = delivery.payload().clone();
        cx.write_to_clipboard(ClipboardItem::new_string(payload.text));
        let outcome = delivery.outcome();
        let tone = match outcome {
            ActionOutcome::CopiedAsFallback { .. } => Tone::Fallback,
            ActionOutcome::Failed { .. } => Tone::Error,
            _ => Tone::Done,
        };
        self.note_used(&record, cx);
        self.set_status(outcome.message(), tone, cx);
    }

    // ---- action handlers ----------------------------------------------------

    fn on_select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn on_select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    fn on_page_prev(&mut self, _: &SelectPagePrev, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-PAGE_ROWS, cx);
    }

    fn on_page_next(&mut self, _: &SelectPageNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(PAGE_ROWS, cx);
    }

    fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Search => {
                if self.selected_remote(cx).is_some() {
                    self.run_remote_action(RemoteAction::CopySourceUrl, cx);
                    return;
                }
                let Some(record) = self.selected_record(cx) else {
                    return;
                };
                if let Some(id) = self.launcher_default(&record) {
                    self.run_action(id, window, cx);
                }
            }
            Mode::Compose => self.send_compose(cx),
            Mode::Sources => self.apply_sources(window, cx),
            Mode::Actions => {
                let Some(entry) = self.menu.as_ref().and_then(|menu| menu.current()) else {
                    return;
                };
                match &entry.availability {
                    ActionAvailability::Available => {
                        let action = entry.action;
                        self.close_menu(window, cx);
                        match action {
                            MenuAction::Local(id) => self.run_action(id, window, cx),
                            MenuAction::Remote(action) => self.run_remote_action(action, cx),
                        }
                    }
                    ActionAvailability::Disabled { reason } => {
                        let reason = reason.clone();
                        self.set_status(reason, Tone::Fallback, cx);
                    }
                }
            }
        }
    }

    fn on_enter_compose(&mut self, _: &EnterCompose, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Search && self.selected_record(cx).is_some() {
            self.enter_compose(window, cx);
        }
    }

    fn on_open_actions(&mut self, _: &OpenActions, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Actions {
            self.close_menu(window, cx);
        } else {
            self.open_actions(window, cx);
        }
    }

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.sources = None;
        self.mode = Mode::Search;
        self.query.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn on_back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        match back_step(self.mode, self.file_view.is_some()) {
            BackStep::CloseActions => self.close_menu(window, cx),
            BackStep::ExitCompose => {
                self.mode = Mode::Search;
                self.query.update(cx, |input, cx| input.focus(window, cx));
                cx.notify();
            }
            BackStep::CloseFile => {
                self.file_view = None;
                self.file_ix = None;
                cx.notify();
            }
            BackStep::Dismiss => cx.emit(LauncherEvent::Dismiss),
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(1, cx);
    }

    fn on_prev_tab(&mut self, _: &PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(-1, cx);
    }

    fn on_next_file(&mut self, _: &NextFile, _: &mut Window, cx: &mut Context<Self>) {
        self.step_file(1, cx);
    }

    fn on_prev_file(&mut self, _: &PrevFile, _: &mut Window, cx: &mut Context<Self>) {
        self.step_file(-1, cx);
    }

    fn request_layout(&mut self, layout: Layout, cx: &mut Context<Self>) {
        if layout != self.layout {
            self.layout = layout;
            cx.emit(LauncherEvent::Relayout(layout));
        }
    }

    fn on_compact(&mut self, _: &UseCompactLayout, _: &mut Window, cx: &mut Context<Self>) {
        self.request_layout(Layout::Compact, cx);
    }

    fn on_split(&mut self, _: &UseSplitLayout, _: &mut Window, cx: &mut Context<Self>) {
        self.request_layout(Layout::Split, cx);
    }

    fn on_expanded(&mut self, _: &UseExpandedLayout, _: &mut Window, cx: &mut Context<Self>) {
        self.request_layout(Layout::Expanded, cx);
    }

    // ---- rendering ----------------------------------------------------------

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let filters: Vec<SharedString> = self
            .parsed
            .kinds
            .iter()
            .map(|kind| SharedString::from(kind.plural_label()))
            .chain(match self.parsed.scope {
                ScopeFilter::All => None,
                ScopeFilter::Local => Some("Local".into()),
                ScopeFilter::Installed => Some("Installed".into()),
                ScopeFilter::Remote => Some("Remote".into()),
            })
            .collect();
        h_flex()
            .h_12()
            .px_3()
            .gap_2()
            .flex_shrink_0()
            .child(
                Icon::new(IconName::Search)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.query).appearance(false)),
            )
            .children(filters.into_iter().map(|label| {
                div()
                    .px_2()
                    .py_0p5()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(palette.hairline)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(label)
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(palette.tertiary)
                    .child(self.result_summary(cx)),
            )
    }

    fn result_summary(&self, cx: &App) -> SharedString {
        match self.catalog.read(cx).state() {
            LoadState::Loading => "Indexing…".into(),
            LoadState::Failed(_) => "Index failed".into(),
            LoadState::Ready { problems, .. } => {
                let count = self.hits.len();
                let results = if count == RESULT_LIMIT {
                    format!("{count}+ results")
                } else if count == 1 {
                    "1 result".to_owned()
                } else {
                    format!("{count} results")
                };
                let mut summary = match problems {
                    0 => results,
                    1 => format!("{results} · 1 scan issue"),
                    n => format!("{results} · {n} scan issues"),
                };
                if let Some(remote) = self.marketplace.read(cx).summary() {
                    summary.push_str(&format!(" · {remote}"));
                }
                summary.into()
            }
        }
    }

    fn render_results(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let row_count = self.rows(cx).len();
        if row_count == 0 {
            let message: SharedString = match self.catalog.read(cx).state() {
                LoadState::Loading => "Indexing local resources…".into(),
                LoadState::Failed(error) => format!("Indexing failed: {error}").into(),
                LoadState::Ready { .. } if self.catalog.read(cx).len() == 0 => {
                    "No resources found in the scanned locations.".into()
                }
                LoadState::Ready { .. } => {
                    let text = self.query.read(cx).value();
                    format!("No matches for “{text}”").into()
                }
            };
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .px_4()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(message),
                )
                .child(div().text_xs().text_color(Palette::get(cx).tertiary).child(
                    "Filter with skill:, mcp:, agent:, prompt:, rule:, plugin: or @installed",
                ))
                .into_any_element();
        }
        uniform_list(
            "launcher-results",
            row_count,
            cx.processor(|this, range: Range<usize>, _window, cx| {
                let rows = this.rows(cx);
                range
                    .filter_map(|ix| {
                        let element = match rows.get(ix)? {
                            RowRef::Local(hit_ix) => {
                                this.render_row(*hit_ix, ix, cx).into_any_element()
                            }
                            RowRef::RemoteHeader => {
                                remote::render_header_row(this.marketplace.read(cx).summary(), cx)
                            }
                            RowRef::Remote(item_ix) => {
                                let item =
                                    this.marketplace.read(cx).merged().get(*item_ix)?.clone();
                                div()
                                    .id(SharedString::from(format!("remote-row-{ix}")))
                                    .w_full()
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.select(ix, cx);
                                    }))
                                    .child(remote::render_remote_row(
                                        &item,
                                        ix == this.selected,
                                        cx,
                                    ))
                                    .into_any_element()
                            }
                        };
                        Some(element)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
        .py_1()
        .into_any_element()
    }

    fn render_row(&self, hit_ix: usize, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let hit = &self.hits[hit_ix];
        let record = self.catalog.read(cx).get(&hit.id);
        let selected = ix == self.selected;
        let (name, source, state, has_issue) = match &record {
            Some(record) => (
                record.name.clone(),
                source_label(record),
                state_label(record),
                record
                    .issues
                    .iter()
                    .any(|issue| issue.severity >= IssueSeverity::Warning),
            ),
            None => (hit.id.key.clone(), String::new(), None, false),
        };
        let highlights: Vec<(Range<usize>, HighlightStyle)> =
            highlight_ranges(&name, &hit.name_match)
                .into_iter()
                .map(|range| {
                    (
                        range,
                        HighlightStyle {
                            font_weight: Some(FontWeight::SEMIBOLD),
                            color: Some(theme.foreground),
                            ..Default::default()
                        },
                    )
                })
                .collect();
        let kind = hit.id.kind;
        h_flex()
            .id(SharedString::from(format!("row-{}", hit.id)))
            .w_full()
            .h(palette.row_height)
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .border_l_2()
            .border_color(if selected {
                palette.focus
            } else {
                theme.transparent
            })
            .when(selected, |row| {
                row.bg(palette.selection)
                    .text_color(palette.selection_foreground)
            })
            .when(!selected, |row| row.hover(|row| row.bg(palette.hover)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.select(ix, cx);
                if event.click_count() >= 2 {
                    this.on_confirm(&Confirm, window, cx);
                }
            }))
            .child(
                Icon::new(kind_icon(kind))
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if selected {
                        palette.selection_foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(StyledText::new(name).with_highlights(highlights)),
            )
            .when(has_issue, |row| {
                row.child(
                    Icon::new(IconName::TriangleAlert)
                        .xsmall()
                        .text_color(palette.warning),
                )
            })
            .when_some(state, |row, state| {
                row.child(div().text_xs().text_color(palette.tertiary).child(state))
            })
            .when(self.layout != Layout::Expanded, |row| {
                row.child(
                    div()
                        .max_w_24()
                        .truncate()
                        .text_xs()
                        .text_color(palette.tertiary)
                        .child(source),
                )
            })
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        if self.mode == Mode::Compose {
            return self.render_compose(cx);
        }
        if let Some(item) = self.selected_remote(cx) {
            return remote::render_remote_preview(&item, cx);
        }
        let Some(record) = self.selected_record(cx) else {
            return div().into_any_element();
        };
        let preview = match &self.preview {
            PreviewSlot::Idle => return div().into_any_element(),
            PreviewSlot::Loading => {
                return v_flex()
                    .size_full()
                    .child(self.render_preview_header(&record, cx))
                    .child(
                        div()
                            .p_4()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Loading preview…"),
                    )
                    .into_any_element();
            }
            PreviewSlot::Failed(message) => {
                return v_flex()
                    .size_full()
                    .child(self.render_preview_header(&record, cx))
                    .child(
                        div()
                            .p_4()
                            .text_sm()
                            .text_color(palette.destructive)
                            .child(message.clone()),
                    )
                    .into_any_element();
            }
            PreviewSlot::Ready(preview) => preview.clone(),
        };
        let tab_ix = self.preview_tab.min(preview.tabs.len().saturating_sub(1));
        let tab_bar = TabBar::new("preview-tabs")
            .underline()
            .small()
            .selected_index(tab_ix)
            .children(
                preview
                    .tabs
                    .iter()
                    .map(|tab| Tab::new().child(tab_label(tab))),
            )
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.preview_tab = *ix;
                cx.notify();
            }));
        let body = preview
            .tabs
            .get(tab_ix)
            .map(|tab| self.render_tab(tab, &preview, cx))
            .unwrap_or_else(|| div().into_any_element());
        v_flex()
            .size_full()
            .min_w_0()
            .child(self.render_preview_header(&record, cx))
            .child(div().px_2().child(tab_bar))
            .child(
                div()
                    .id("preview-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .py_3()
                    .child(body),
            )
            .into_any_element()
    }

    fn render_preview_header(&self, record: &ResourceRecord, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let path = record
            .local_path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        v_flex()
            .px_4()
            .pt_3()
            .pb_1()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Icon::new(kind_icon(record.kind()))
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(record.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.tertiary)
                            .child(record.kind().label()),
                    ),
            )
            .when(!record.summary.is_empty(), |el| {
                el.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .line_clamp(2)
                        .child(record.summary.clone()),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(palette.tertiary)
                    .truncate()
                    .child(path),
            )
    }

    fn render_tab(
        &self,
        tab: &PreviewTab,
        preview: &perkolator_preview::Preview,
        cx: &App,
    ) -> AnyElement {
        let id_base = format!("{}-{}", preview.generation, self.preview_tab);
        match tab {
            PreviewTab::Overview(document) => render_document(&id_base, document, cx),
            PreviewTab::Files(tree) => self.render_files(tree, &id_base, cx),
            PreviewTab::Config { format, text } => {
                markdown_view(&id_base, fence(format, text)).into_any_element()
            }
            PreviewTab::Tools(tools) => {
                let mut markdown = String::new();
                for tool in tools {
                    markdown.push_str(&format!("### `{}`\n\n", tool.name));
                    if let Some(description) = &tool.description {
                        markdown.push_str(&format!("{description}\n\n"));
                    }
                    for param in &tool.parameters {
                        markdown.push_str(&format!(
                            "- `{}`{}{}{}\n",
                            param.name,
                            param
                                .type_name
                                .as_deref()
                                .map(|t| format!(": {t}"))
                                .unwrap_or_default(),
                            if param.required { " (required)" } else { "" },
                            param
                                .description
                                .as_deref()
                                .map(|d| format!(" — {d}"))
                                .unwrap_or_default(),
                        ));
                    }
                    markdown.push('\n');
                }
                markdown_view(&id_base, markdown).into_any_element()
            }
            PreviewTab::Template { variables, raw } => {
                let mut markdown = String::new();
                if !variables.is_empty() {
                    markdown.push_str(
                        "| Variable | Required | Default | Description |\n|---|---|---|---|\n",
                    );
                    for variable in variables {
                        markdown.push_str(&format!(
                            "| `{}` | {} | {} | {} |\n",
                            variable.name,
                            if variable.required { "yes" } else { "no" },
                            variable.default.as_deref().unwrap_or("—"),
                            variable.description.as_deref().unwrap_or(""),
                        ));
                    }
                    markdown.push('\n');
                }
                markdown.push_str(&fence("", raw));
                markdown_view(&id_base, markdown).into_any_element()
            }
            PreviewTab::Precedence(rows) | PreviewTab::Metadata(rows) => {
                render_key_values(rows, cx)
            }
            PreviewTab::Permissions {
                permissions,
                executable_files,
                content_hash,
            } => {
                let mut rows: Vec<(String, String)> = Vec::new();
                rows.push((
                    "Declared permissions".into(),
                    if permissions.is_empty() {
                        "None declared".into()
                    } else {
                        permissions.join(", ")
                    },
                ));
                rows.push((
                    "Executable files".into(),
                    if executable_files.is_empty() {
                        "None".into()
                    } else {
                        executable_files
                            .iter()
                            .map(RelPath::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    },
                ));
                if let Some(hash) = content_hash {
                    rows.push(("Content hash".into(), hash.clone()));
                }
                rows.push((
                    "Execution".into(),
                    "Nothing runs during preview. Running plugin code needs explicit approval."
                        .into(),
                ));
                render_key_values(&rows, cx)
            }
        }
    }

    fn render_files(
        &self,
        tree: &perkolator_preview::FileTree,
        id_base: &str,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let rows = tree.nodes.iter().enumerate().map(|(ix, node)| {
            let selected = self.file_ix == Some(ix);
            h_flex()
                .h_6()
                .pl(gpui_kit::rems(0.25 + node.depth as f32 * 1.0))
                .pr_2()
                .gap_1p5()
                .rounded(theme.radius)
                .when(selected, |row| row.bg(palette.selection))
                .child(
                    Icon::new(if node.is_dir {
                        IconName::Folder
                    } else {
                        file_icon(&node.kind)
                    })
                    .xsmall()
                    .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(if selected {
                            palette.selection_foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(node.name.clone()),
                )
        });
        let file = self.file_view.as_ref().map(|view| match &view.document {
            None => div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Loading file…")
                .into_any_element(),
            Some(document) => v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(palette.tertiary)
                        .child(view.path.to_string()),
                )
                .child(render_document(
                    &format!("{id_base}-{}", view.path),
                    document,
                    cx,
                ))
                .into_any_element(),
        });
        v_flex()
            .gap_3()
            .child(v_flex().children(rows))
            .when(tree.truncated, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(palette.tertiary)
                        .child("Tree truncated at the preview limit"),
                )
            })
            .when_some(file, |el, file| {
                el.child(div().h_px().bg(palette.hairline)).child(file)
            })
            .into_any_element()
    }

    fn render_compose(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let Some(record) = self.selected_record(cx) else {
            return div().into_any_element();
        };
        let payload = self.composed_text(&record, cx);
        let insert = self
            .features
            .get(&PlatformFeature::AgentSessionInsert)
            .cloned()
            .unwrap_or(FeatureSupport::Unknown);
        let delivery_note: SharedString = match insert {
            FeatureSupport::Supported => "Enter inserts into the captured session".into(),
            other => format!(
                "Enter copies to the clipboard — {}",
                other
                    .explanation()
                    .unwrap_or("session insertion not available")
            )
            .into(),
        };
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Compose with"),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .px_2()
                            .py_0p5()
                            .rounded(theme.radius)
                            .bg(palette.raised)
                            .child(Icon::new(kind_icon(record.kind())).xsmall())
                            .child(div().text_sm().child(record.name.clone())),
                    ),
            )
            .child(
                div()
                    .border_1()
                    .border_color(palette.hairline)
                    .rounded(theme.radius)
                    .px_2()
                    .child(Input::new(&self.compose).appearance(false)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette.tertiary)
                    .child("Exact text"),
            )
            .child(
                div()
                    .id("compose-payload")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(markdown_view("compose-payload-md", fence("", &payload))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette.tertiary)
                    .child(delivery_note),
            )
            .into_any_element()
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.menu.as_ref()?;
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let rows = menu.entries.iter().enumerate().map(|(ix, entry)| {
            let selected = ix == menu.selected;
            let disabled_reason = match &entry.availability {
                ActionAvailability::Available => None,
                ActionAvailability::Disabled { reason } => Some(reason.clone()),
            };
            v_flex()
                .px_2()
                .py_1()
                .rounded(theme.radius)
                .when(selected, |row| row.bg(palette.selection))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(if disabled_reason.is_some() {
                                    palette.tertiary
                                } else {
                                    theme.foreground
                                })
                                .child(entry.label),
                        )
                        .when_some(entry.shortcut_hint, |row, hint| {
                            row.child(div().text_xs().text_color(palette.tertiary).child(hint))
                        }),
                )
                .when_some(disabled_reason.filter(|_| selected), |row, reason| {
                    row.child(div().text_xs().text_color(palette.tertiary).child(reason))
                })
        });
        Some(
            div()
                .absolute()
                .bottom_10()
                .right_3()
                .w_80()
                .p_1()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .pb_1()
                        .text_xs()
                        .text_color(palette.tertiary)
                        .child("Actions"),
                )
                .child(v_flex().children(rows)),
        )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let default_label = if self.selected_remote(cx).is_some() {
            "Copy source URL"
        } else {
            self.selected_record(cx)
                .and_then(|record| self.launcher_default(&record))
                .map(ActionId::label)
                .unwrap_or("Run")
        };
        let hints: Vec<(&'static str, SharedString)> = match self.mode {
            Mode::Search => vec![
                ("enter", default_label.into()),
                ("tab", "Compose".into()),
                (ACTIONS_KEY, "Actions".into()),
                ("ctrl-tab", "Next tab".into()),
                (SOURCES_KEY, "Sources".into()),
                ("escape", "Close".into()),
            ],
            Mode::Compose => vec![
                ("enter", "Copy composed text".into()),
                ("escape", "Back".into()),
            ],
            Mode::Actions => vec![("enter", "Run".into()), ("escape", "Back".into())],
            Mode::Sources => vec![
                ("space", "Toggle".into()),
                ("enter", "Apply".into()),
                ("escape", "Cancel".into()),
            ],
        };
        let status = self.status.as_ref().map(|status| {
            let color = match status.tone {
                Tone::Done => theme.muted_foreground,
                Tone::Fallback => palette.warning,
                Tone::Error => palette.destructive,
            };
            h_flex()
                .gap_1()
                .min_w_0()
                .text_color(color)
                .child(
                    Icon::new(match status.tone {
                        Tone::Done => IconName::Check,
                        Tone::Fallback => IconName::Info,
                        Tone::Error => IconName::CircleAlert,
                    })
                    .xsmall(),
                )
                .child(div().truncate().child(status.text.clone()))
        });
        h_flex()
            .h_8()
            .px_3()
            .gap_3()
            .flex_shrink_0()
            .bg(palette.chrome)
            .text_xs()
            .text_color(theme.muted_foreground)
            .children(hints.into_iter().map(|(key, label)| {
                h_flex()
                    .gap_1()
                    .when_some(Keystroke::parse(key).ok(), |el, keystroke| {
                        el.child(Kbd::new(keystroke))
                    })
                    .child(label)
            }))
            .child(div().flex_1())
            .children(status)
    }
}

impl Render for LauncherView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette::get(cx);
        let fraction = self.layout.results_fraction();
        let shows_preview = self.layout.shows_preview();
        v_flex()
            .id("launcher")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .border_1()
            .border_color(palette.hairline)
            .on_action(cx.listener(Self::on_select_prev))
            .on_action(cx.listener(Self::on_select_next))
            .on_action(cx.listener(Self::on_page_prev))
            .on_action(cx.listener(Self::on_page_next))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_enter_compose))
            .on_action(cx.listener(Self::on_open_actions))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_next_file))
            .on_action(cx.listener(Self::on_prev_file))
            .on_action(cx.listener(Self::on_compact))
            .on_action(cx.listener(Self::on_split))
            .on_action(cx.listener(Self::on_expanded))
            .on_action(cx.listener(Self::on_open_sources))
            .on_action(cx.listener(Self::on_toggle_source))
            .child(self.render_header(cx))
            .child(div().h_px().flex_shrink_0().bg(palette.hairline))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        div()
                            .h_full()
                            .min_w_0()
                            .when(shows_preview, |el| el.w(gpui_kit::relative(fraction)))
                            .when(!shows_preview, |el| el.flex_1())
                            .child(self.render_results(cx)),
                    )
                    .when(shows_preview, |el| {
                        el.child(div().w_px().h_full().bg(palette.hairline)).child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .child(self.render_preview(cx)),
                        )
                    }),
            )
            .child(div().h_px().flex_shrink_0().bg(palette.hairline))
            .child(self.render_footer(cx))
            .children(self.render_menu(cx))
            .children(
                self.sources
                    .as_ref()
                    .map(|menu| remote::render_source_menu(menu, cx)),
            )
    }
}

/// Link for a remote item: canonical page first, else its repository.
fn remote_url(item: &MergedItem) -> Option<String> {
    item.sources().find_map(|record| {
        record
            .canonical_url
            .clone()
            .or_else(|| record.repo_url.clone())
    })
}

fn markdown_view(id: impl Into<SharedString>, text: String) -> TextView {
    let id: SharedString = id.into();
    TextView::markdown(id, text).selectable(true)
}

fn render_document(id_base: &str, document: &Document, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let palette = Palette::get(cx);
    let muted = |text: String| {
        div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(text)
            .into_any_element()
    };
    match document {
        Document::Markdown {
            text, frontmatter, ..
        } => v_flex()
            .gap_3()
            .when(!frontmatter.is_empty(), |el| {
                el.child(render_key_values(frontmatter, cx))
                    .child(div().h_px().bg(palette.hairline))
            })
            .child(markdown_view(format!("{id_base}-md"), text.clone()))
            .into_any_element(),
        Document::Code { language, text } => {
            markdown_view(format!("{id_base}-code"), fence(language, text)).into_any_element()
        }
        Document::PlainText(text) => {
            markdown_view(format!("{id_base}-text"), fence("", text)).into_any_element()
        }
        Document::Image { path } => div()
            .max_h_96()
            .child(img(path.clone()).max_w_full().max_h_96())
            .into_any_element(),
        Document::Binary { size } => muted(format!(
            "Binary file · {} — not previewed",
            human_size(*size)
        )),
        Document::TooLarge { size, limit } => muted(format!(
            "File is {}; the preview limit is {}",
            human_size(*size),
            human_size(*limit)
        )),
        Document::Unavailable { reason } => muted(reason.clone()),
    }
}

fn render_key_values(rows: &[(String, String)], cx: &App) -> AnyElement {
    let theme = cx.theme();
    let palette = Palette::get(cx);
    v_flex()
        .gap_1()
        .children(rows.iter().map(|(key, value)| {
            h_flex()
                .items_start()
                .gap_3()
                .child(
                    div()
                        .w_32()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(palette.tertiary)
                        .child(key.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(value.clone()),
                )
        }))
        .into_any_element()
}

fn tab_label(tab: &PreviewTab) -> &'static str {
    match tab {
        PreviewTab::Overview(_) => "Overview",
        PreviewTab::Files(_) => "Files",
        PreviewTab::Config { .. } => "Config",
        PreviewTab::Tools(_) => "Tools",
        PreviewTab::Template { .. } => "Template",
        PreviewTab::Precedence(_) => "Scope",
        PreviewTab::Permissions { .. } => "Permissions",
        PreviewTab::Metadata(_) => "Metadata",
    }
}

pub(super) fn kind_icon(kind: ResourceKind) -> IconName {
    match kind {
        ResourceKind::Skill => IconName::Sparkles,
        ResourceKind::McpServer => IconName::Server,
        ResourceKind::Agent => IconName::Bot,
        ResourceKind::Prompt => IconName::MessageSquareText,
        ResourceKind::Rule => IconName::ScrollText,
        ResourceKind::Plugin => IconName::Puzzle,
    }
}

fn file_icon(kind: &perkolator_core::FileKind) -> IconName {
    use perkolator_core::FileKind;
    match kind {
        FileKind::Markdown | FileKind::Text => IconName::FileText,
        FileKind::Code { .. } | FileKind::Json | FileKind::Toml | FileKind::Yaml => {
            IconName::FileCode
        }
        FileKind::Image => IconName::Image,
        FileKind::Symlink { .. } => IconName::Link,
        FileKind::Directory => IconName::Folder,
        FileKind::Binary => IconName::File,
    }
}

fn source_label(record: &ResourceRecord) -> String {
    match &record.provenance.source {
        SourceKind::Library { .. } => "Library".into(),
        SourceKind::AgentHost { host, .. } => host.clone(),
        SourceKind::Project { root } => root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Project".into()),
        SourceKind::Remote { provider, .. } => provider.clone(),
        SourceKind::Cache { provider, .. } => format!("{provider} (cached)"),
    }
}

/// The most informative state worth showing in a dense row.
fn state_label(record: &ResourceRecord) -> Option<String> {
    let deployments = record.deployments().count();
    if deployments > 0 {
        let linked = record
            .locations
            .iter()
            .filter(|location| location.state == InstallState::Linked)
            .count();
        return Some(if linked == deployments {
            format!("Linked ×{deployments}")
        } else {
            format!("Deployed ×{deployments}")
        });
    }
    record
        .locations
        .iter()
        .find(|location| !matches!(location.role, LocationRole::Canonical))
        .map(|location| location.state.label().to_owned())
        .filter(|label| label != "Unknown")
}
