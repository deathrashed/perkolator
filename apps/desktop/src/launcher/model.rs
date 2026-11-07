//! GPUI-free launcher state rules, unit-tested in isolation.

use std::ops::Range;

use perkolator_actions::ActionEntry;
use perkolator_core::{ActionAvailability, ActionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Search,
    Compose,
    Actions,
    Sources,
}

/// One row of the combined result list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowRef {
    Local(usize),
    RemoteHeader,
    Remote(usize),
}

impl RowRef {
    pub fn is_selectable(self) -> bool {
        self != RowRef::RemoteHeader
    }
}

/// Local hits first, then (when present) a header and merged remote items.
pub fn build_rows(local: usize, remote: usize) -> Vec<RowRef> {
    let mut rows: Vec<RowRef> = (0..local).map(RowRef::Local).collect();
    if remote > 0 {
        rows.push(RowRef::RemoteHeader);
        rows.extend((0..remote).map(RowRef::Remote));
    }
    rows
}

/// Step `delta` selectable rows from `current`, clamped at the ends.
pub fn step_row(rows: &[RowRef], current: usize, delta: isize) -> usize {
    let selectable: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.is_selectable())
        .map(|(ix, _)| ix)
        .collect();
    if selectable.is_empty() {
        return 0;
    }
    let position = selectable
        .iter()
        .position(|ix| *ix >= current)
        .unwrap_or(selectable.len() - 1);
    selectable[step_selection(position, selectable.len(), delta)]
}

/// Actions for remote marketplace items. Nothing here installs or runs code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAction {
    CopySourceUrl,
    OpenInBrowser,
    CopyPackageRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Local(ActionId),
    Remote(RemoteAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: MenuAction,
    pub label: &'static str,
    pub shortcut_hint: Option<&'static str>,
    pub availability: ActionAvailability,
}

impl From<ActionEntry> for MenuEntry {
    fn from(entry: ActionEntry) -> Self {
        Self {
            action: MenuAction::Local(entry.id),
            label: entry.label,
            shortcut_hint: entry.shortcut_hint,
            availability: entry.availability,
        }
    }
}

/// Remote actions offered for an item, given which links it really has.
pub fn remote_entries(has_url: bool, has_package: bool) -> Vec<MenuEntry> {
    let entry = |action, label, available: bool, reason: &str| MenuEntry {
        action: MenuAction::Remote(action),
        label,
        shortcut_hint: None,
        availability: if available {
            ActionAvailability::Available
        } else {
            ActionAvailability::Disabled {
                reason: reason.to_owned(),
            }
        },
    };
    let mut entries = vec![
        entry(
            RemoteAction::CopySourceUrl,
            "Copy source URL",
            has_url,
            "The source did not provide a URL",
        ),
        entry(
            RemoteAction::OpenInBrowser,
            "Open in browser",
            has_url,
            "The source did not provide a URL",
        ),
    ];
    if has_package {
        entries.push(entry(
            RemoteAction::CopyPackageRef,
            "Copy package reference",
            true,
            "",
        ));
    }
    entries
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    Compact,
    #[default]
    Split,
    Expanded,
}

impl Layout {
    /// Logical window size (width, height).
    pub fn size(self) -> (f32, f32) {
        match self {
            Layout::Compact => (680., 440.),
            Layout::Split => (1040., 620.),
            Layout::Expanded => (1180., 760.),
        }
    }

    pub fn shows_preview(self) -> bool {
        self != Layout::Compact
    }

    /// Fraction of the body width given to the result list.
    pub fn results_fraction(self) -> f32 {
        match self {
            Layout::Compact => 1.0,
            Layout::Split => 0.42,
            Layout::Expanded => 0.34,
        }
    }
}

/// What Escape does next: close the topmost surface, then step back, then
/// dismiss. The query is never cleared by Escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackStep {
    CloseActions,
    ExitCompose,
    CloseFile,
    Dismiss,
}

pub fn back_step(mode: Mode, file_open: bool) -> BackStep {
    match mode {
        Mode::Actions | Mode::Sources => BackStep::CloseActions,
        Mode::Compose => BackStep::ExitCompose,
        Mode::Search if file_open => BackStep::CloseFile,
        Mode::Search => BackStep::Dismiss,
    }
}

/// Actions the launcher can perform today. Others are hidden rather than
/// shown as working buttons.
pub fn launcher_supports(id: ActionId) -> bool {
    !matches!(id, ActionId::ShowDetails)
}

pub struct ActionMenu {
    pub entries: Vec<MenuEntry>,
    pub selected: usize,
}

impl ActionMenu {
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        Self {
            entries,
            selected: 0,
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
    }

    pub fn current(&self) -> Option<&MenuEntry> {
        self.entries.get(self.selected)
    }
}

/// Move a list selection by `delta`, clamped (no wrap) like native lists.
pub fn step_selection(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as isize + delta).clamp(0, len as isize - 1) as usize
}

/// Convert sorted char indices (from the matcher) into merged byte ranges.
pub fn highlight_ranges(text: &str, char_indices: &[u32]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = char_indices.iter().copied().peekable();
    for (char_ix, (byte_ix, ch)) in text.char_indices().enumerate() {
        let Some(&next) = wanted.peek() else { break };
        if next as usize != char_ix {
            continue;
        }
        wanted.next();
        let end = byte_ix + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == byte_ix => last.end = end,
            _ => ranges.push(byte_ix..end),
        }
    }
    ranges
}

/// Wrap text in a Markdown code fence that cannot be closed by the content.
pub fn fence(language: &str, text: &str) -> String {
    let longest_run = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let ticks = "`".repeat(longest_run.max(2) + 1);
    let newline = if text.ends_with('\n') { "" } else { "\n" };
    format!("{ticks}{language}\n{text}{newline}{ticks}\n")
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_chain_closes_top_surface_first_and_never_clears_query() {
        assert_eq!(back_step(Mode::Actions, true), BackStep::CloseActions);
        assert_eq!(back_step(Mode::Compose, false), BackStep::ExitCompose);
        assert_eq!(back_step(Mode::Search, true), BackStep::CloseFile);
        assert_eq!(back_step(Mode::Search, false), BackStep::Dismiss);
    }

    #[test]
    fn selection_clamps() {
        assert_eq!(step_selection(0, 3, -1), 0);
        assert_eq!(step_selection(2, 3, 1), 2);
        assert_eq!(step_selection(1, 3, 1), 2);
        assert_eq!(step_selection(5, 0, 1), 0);
    }

    #[test]
    fn action_menu_wraps() {
        let entry = |id| {
            MenuEntry::from(ActionEntry {
                id,
                label: "x",
                shortcut_hint: None,
                availability: ActionAvailability::Available,
            })
        };
        let mut menu = ActionMenu::new(vec![
            entry(ActionId::CopyPath),
            entry(ActionId::CopyContent),
        ]);
        menu.move_by(-1);
        assert_eq!(
            menu.current().unwrap().action,
            MenuAction::Local(ActionId::CopyContent)
        );
        menu.move_by(1);
        assert_eq!(
            menu.current().unwrap().action,
            MenuAction::Local(ActionId::CopyPath)
        );
    }

    #[test]
    fn rows_skip_the_remote_header() {
        let rows = build_rows(2, 2);
        assert_eq!(
            rows,
            vec![
                RowRef::Local(0),
                RowRef::Local(1),
                RowRef::RemoteHeader,
                RowRef::Remote(0),
                RowRef::Remote(1)
            ]
        );
        assert_eq!(
            step_row(&rows, 1, 1),
            3,
            "down from last local skips header"
        );
        assert_eq!(
            step_row(&rows, 3, -1),
            1,
            "up from first remote skips header"
        );
        assert_eq!(step_row(&rows, 4, 1), 4);
        assert_eq!(build_rows(0, 0), vec![]);
        assert_eq!(build_rows(1, 0), vec![RowRef::Local(0)]);
    }

    #[test]
    fn remote_actions_never_offer_install_and_explain_missing_urls() {
        let entries = remote_entries(false, false);
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| matches!(entry.availability, ActionAvailability::Disabled { .. }))
        );
        assert_eq!(remote_entries(true, true).len(), 3);
    }

    #[test]
    fn highlight_merges_adjacent_and_handles_unicode() {
        assert_eq!(
            highlight_ranges("frontend", &[0, 1, 2, 5]),
            vec![0..3, 5..6]
        );
        assert_eq!(highlight_ranges("éa", &[1]), vec![2..3]);
        assert!(highlight_ranges("abc", &[]).is_empty());
    }

    #[test]
    fn fence_survives_backticks_in_content() {
        let fenced = fence("md", "use ``` here");
        assert!(fenced.starts_with("````md\n"));
        assert!(fenced.ends_with("````\n"));
    }

    #[test]
    fn sizes_are_human() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn layouts_have_distinct_sizes() {
        assert!(!Layout::Compact.shows_preview());
        assert!(Layout::Split.shows_preview());
        assert!(Layout::Expanded.size().0 > Layout::Split.size().0);
    }
}
