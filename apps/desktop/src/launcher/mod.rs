//! The global quick launcher: a resident controller that opens a focused
//! overlay window on demand and remembers query, selection and layout
//! between invocations.

mod model;
mod view;

use std::time::Instant;

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, KeyBinding, Subscription,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions,
    actions, px, size,
};
use perkolator_core::ResourceId;

use crate::catalog::Catalog;
use crate::platform;
pub use model::Layout;
pub use view::{LauncherEvent, LauncherView};

actions!(
    perkolator_launcher,
    [
        SelectPrev,
        SelectNext,
        SelectPagePrev,
        SelectPageNext,
        Confirm,
        EnterCompose,
        Back,
        OpenActions,
        NextTab,
        PrevTab,
        NextFile,
        PrevFile,
        UseCompactLayout,
        UseSplitLayout,
        UseExpandedLayout,
    ]
);

pub const CONTEXT: &str = "PerkLauncher";

/// Platform command-modifier keystrokes (this GPUI has no `secondary-`).
#[cfg(target_os = "macos")]
mod keys {
    pub const ACTIONS: &str = "cmd-k";
    pub const ACTIONS_ALT: &str = "cmd-enter";
    pub const COMPACT: &str = "cmd-1";
    pub const SPLIT: &str = "cmd-2";
    pub const EXPANDED: &str = "cmd-3";
}
#[cfg(not(target_os = "macos"))]
mod keys {
    pub const ACTIONS: &str = "ctrl-k";
    pub const ACTIONS_ALT: &str = "ctrl-enter";
    pub const COMPACT: &str = "ctrl-1";
    pub const SPLIT: &str = "ctrl-2";
    pub const EXPANDED: &str = "ctrl-3";
}
pub use keys::ACTIONS as ACTIONS_KEY;

/// Bind after `gpui_kit::init`: bindings at the same context depth that are
/// added later win, so `PerkLauncher > Input` overrides the text field's own
/// up/down/enter/escape/tab while typing.
pub fn bind_keys(cx: &mut App) {
    let mut bindings = Vec::new();
    for context in [CONTEXT, "PerkLauncher > Input"] {
        bindings.extend([
            KeyBinding::new("up", SelectPrev, Some(context)),
            KeyBinding::new("down", SelectNext, Some(context)),
            KeyBinding::new("pageup", SelectPagePrev, Some(context)),
            KeyBinding::new("pagedown", SelectPageNext, Some(context)),
            KeyBinding::new("enter", Confirm, Some(context)),
            KeyBinding::new("tab", EnterCompose, Some(context)),
            KeyBinding::new("escape", Back, Some(context)),
            KeyBinding::new(keys::ACTIONS, OpenActions, Some(context)),
            KeyBinding::new(keys::ACTIONS_ALT, OpenActions, Some(context)),
            KeyBinding::new("ctrl-tab", NextTab, Some(context)),
            KeyBinding::new("ctrl-shift-tab", PrevTab, Some(context)),
            KeyBinding::new("ctrl-down", NextFile, Some(context)),
            KeyBinding::new("ctrl-up", PrevFile, Some(context)),
            KeyBinding::new(keys::COMPACT, UseCompactLayout, Some(context)),
            KeyBinding::new(keys::SPLIT, UseSplitLayout, Some(context)),
            KeyBinding::new(keys::EXPANDED, UseExpandedLayout, Some(context)),
        ]);
    }
    cx.bind_keys(bindings);
}

/// What survives closing the launcher window.
#[derive(Clone, Debug, Default)]
pub struct LauncherMemory {
    pub query: String,
    pub selected: Option<ResourceId>,
    pub layout: Layout,
}

struct OpenLauncher {
    handle: AnyWindowHandle,
    view: Entity<LauncherView>,
    _events: Subscription,
}

pub struct LauncherController {
    catalog: Entity<Catalog>,
    open: Option<OpenLauncher>,
    memory: LauncherMemory,
}

impl LauncherController {
    pub fn new(catalog: Entity<Catalog>) -> Self {
        Self {
            catalog,
            open: None,
            memory: LauncherMemory::default(),
        }
    }

    pub fn show(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = &self.open {
            let activated = open
                .handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok();
            if activated {
                return;
            }
            self.open = None;
        }
        self.open_window(cx);
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        let is_open = self
            .open
            .as_ref()
            .is_some_and(|open| open.handle.update(cx, |_, _, _| ()).is_ok());
        if is_open {
            self.hide(cx);
        } else {
            self.open = None;
            self.open_window(cx);
        }
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.open.take() {
            self.memory = open.view.read(cx).memory(cx);
            let _ = open
                .handle
                .update(cx, |_, window, _| window.remove_window());
        }
    }

    fn relayout(&mut self, layout: Layout, cx: &mut Context<Self>) {
        if self.memory.layout == layout && self.open.is_some() {
            return;
        }
        self.hide(cx);
        self.memory.layout = layout;
        self.open_window(cx);
    }

    fn open_window(&mut self, cx: &mut Context<Self>) {
        let started = Instant::now();
        let use_layer_shell = platform::prefers_layer_shell();
        let result = self.try_open(use_layer_shell, cx).or_else(|error| {
            if use_layer_shell {
                eprintln!(
                    "perkolator: layer-shell launcher failed ({error}); using a popup window"
                );
                self.try_open(false, cx)
            } else {
                Err(error)
            }
        });
        match result {
            Ok(open) => {
                if std::env::var_os("PERKOLATOR_DEBUG").is_some() {
                    eprintln!(
                        "perkolator: launcher window opened in {:.1} ms (layer-shell: {use_layer_shell})",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
                self.open = Some(open);
            }
            Err(error) => eprintln!("perkolator: cannot open launcher window: {error}"),
        }
    }

    fn try_open(
        &mut self,
        layer_shell: bool,
        cx: &mut Context<Self>,
    ) -> Result<OpenLauncher, String> {
        let options = window_options(self.memory.layout, layer_shell, cx);
        let catalog = self.catalog.clone();
        let memory = self.memory.clone();
        let (handle, view) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| LauncherView::new(catalog, memory, window, cx))
        })
        .map_err(|error| error.to_string())?;
        let events = cx.subscribe(
            &view,
            |controller, _, event: &LauncherEvent, cx| match event {
                LauncherEvent::Dismiss => controller.hide(cx),
                LauncherEvent::Relayout(layout) => controller.relayout(*layout, cx),
            },
        );
        Ok(OpenLauncher {
            handle,
            view,
            _events: events,
        })
    }
}

fn keyboard_interactivity() -> KeyboardInteractivity {
    match std::env::var("PERKOLATOR_LAYER_KEYBOARD").as_deref() {
        Ok("on-demand") => KeyboardInteractivity::OnDemand,
        _ => KeyboardInteractivity::Exclusive,
    }
}

/// Window geometry is a platform boundary, so logical pixel sizes are used
/// directly here.
fn window_options(layout: Layout, layer_shell: bool, cx: &App) -> WindowOptions {
    let (width, height) = layout.size();
    let window_size = size(px(width), px(height));
    let bounds = Bounds::centered(None, window_size, cx);
    let kind = if layer_shell {
        let screen_height = cx
            .primary_display()
            .map(|display| f32::from(display.bounds().size.height))
            .unwrap_or(height * 2.0);
        let top = ((screen_height - height) / 3.0).max(48.0);
        WindowKind::LayerShell(LayerShellOptions {
            namespace: "perkolator-launcher".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP,
            exclusive_zone: None,
            exclusive_edge: None,
            margin: Some((px(top), px(0.), px(0.), px(0.))),
            keyboard_interactivity: keyboard_interactivity(),
        })
    } else {
        WindowKind::PopUp
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: true,
        show: true,
        kind,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        app_id: Some(platform::APP_ID.into()),
        window_background: WindowBackgroundAppearance::Opaque,
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}
