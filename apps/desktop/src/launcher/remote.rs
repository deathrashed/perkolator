//! Rendering for marketplace results and the source selector popover.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use perkolator_core::ProviderId;
use perkolator_marketplace::config::MarketplaceSettings;
use perkolator_marketplace::dedup::MergedItem;

use super::view::kind_icon;
use crate::marketplace::SourceRow;
use crate::theme_bridge::Palette;

/// Editable copy of the source settings; applied on Enter, dropped on Esc.
pub struct SourceMenu {
    pub draft: MarketplaceSettings,
    pub rows: Vec<SourceMenuRow>,
    pub selected: usize,
}

pub struct SourceMenuRow {
    /// `None` is the master "Search marketplaces" switch.
    pub id: Option<ProviderId>,
    pub name: String,
    pub detail: String,
}

impl SourceMenu {
    pub fn new(draft: MarketplaceSettings, sources: &[SourceRow]) -> Self {
        let mut rows = vec![SourceMenuRow {
            id: None,
            name: "Search marketplaces".into(),
            detail: "Send queries to the enabled sources below over the network".into(),
        }];
        rows.extend(sources.iter().map(|source| {
            let kinds: Vec<&str> = source
                .descriptor
                .kinds
                .iter()
                .map(|kind| kind.plural_label())
                .collect();
            let state = source.state.label();
            let mut detail = format!(
                "{} · {} · {}",
                kinds.join(", "),
                source.descriptor.transport.label(),
                source.auth.label()
            );
            if !state.is_empty() {
                detail.push_str(&format!(" · {state}"));
            }
            SourceMenuRow {
                id: Some(source.descriptor.id.clone()),
                name: source.descriptor.display_name.clone(),
                detail,
            }
        }));
        Self {
            draft,
            rows,
            selected: 0,
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        let len = self.rows.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len.max(1)) as usize;
    }

    pub fn is_checked(&self, ix: usize) -> bool {
        match self.rows.get(ix).and_then(|row| row.id.as_ref()) {
            None => self.draft.remote_search_enabled,
            Some(id) => self.draft.is_enabled(id),
        }
    }

    pub fn toggle_selected(&mut self) {
        let checked = self.is_checked(self.selected);
        match self.rows.get(self.selected).and_then(|row| row.id.clone()) {
            None => self.draft.remote_search_enabled = !checked,
            Some(id) => self.draft.set_enabled(id, !checked),
        }
    }
}

pub fn render_header_row(summary: Option<String>, cx: &App) -> AnyElement {
    let palette = Palette::get(cx);
    h_flex()
        .w_full()
        .h(palette.row_height)
        .px_3()
        .gap_2()
        .text_xs()
        .text_color(palette.tertiary)
        .child("Marketplaces")
        .child(div().flex_1().h_px().bg(palette.hairline))
        .children(summary)
        .into_any_element()
}

pub fn render_remote_row(item: &MergedItem, selected: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let palette = Palette::get(cx);
    let primary = &item.primary;
    let sources: Vec<String> = item
        .sources()
        .map(|record| record.source.to_string())
        .collect();
    h_flex()
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
        .when(selected, |row| row.bg(palette.selection))
        .child(
            Icon::new(kind_icon(primary.kind))
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
                .child(primary.title.clone()),
        )
        .child(
            div()
                .max_w_32()
                .truncate()
                .text_xs()
                .text_color(palette.tertiary)
                .child(sources.join(", ")),
        )
        .into_any_element()
}

pub fn render_remote_preview(item: &MergedItem, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let palette = Palette::get(cx);
    let primary = &item.primary;
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push(("Kind".into(), primary.kind.label().into()));
    for record in item.sources() {
        let link = record
            .canonical_url
            .clone()
            .or_else(|| record.repo_url.clone())
            .unwrap_or_else(|| "No link provided".into());
        rows.push((format!("Source: {}", record.source), link));
    }
    if let Some(path) = &primary.resource_path {
        rows.push(("Path in repository".into(), path.clone()));
    }
    if let Some(package) = &primary.package_ref {
        rows.push(("Package".into(), package.clone()));
    }
    if let Some(version) = &primary.version {
        rows.push(("Version".into(), version.clone()));
    }
    if let Some(author) = &primary.author {
        rows.push(("Author".into(), author.clone()));
    }
    if !primary.categories.is_empty() {
        rows.push(("Categories".into(), primary.categories.join(", ")));
    }
    v_flex()
        .size_full()
        .px_4()
        .pt_3()
        .gap_3()
        .child(
            h_flex()
                .gap_2()
                .child(
                    Icon::new(kind_icon(primary.kind))
                        .small()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(primary.title.clone()),
                ),
        )
        .when(!primary.summary.is_empty(), |el| {
            el.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(primary.summary.clone()),
            )
        })
        .child(
            v_flex()
                .gap_1()
                .children(rows.into_iter().map(|(key, value)| {
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .w_32()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(palette.tertiary)
                                .child(key),
                        )
                        .child(div().flex_1().min_w_0().text_sm().child(value))
                })),
        )
        .child(
            div()
                .text_xs()
                .text_color(palette.tertiary)
                .child("Remote metadata. Nothing is downloaded, installed or run from this view."),
        )
        .into_any_element()
}

pub fn render_source_menu(menu: &SourceMenu, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let palette = Palette::get(cx);
    let rows = menu.rows.iter().enumerate().map(|(ix, row)| {
        let selected = ix == menu.selected;
        let checked = menu.is_checked(ix);
        let dimmed = row.id.is_some() && !menu.draft.remote_search_enabled;
        h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .rounded(theme.radius)
            .when(selected, |el| el.bg(palette.selection))
            .child(
                Icon::new(if checked {
                    IconName::SquareCheck
                } else {
                    IconName::Square
                })
                .small()
                .text_color(if checked {
                    theme.foreground
                } else {
                    palette.tertiary
                }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .text_color(if dimmed {
                                palette.tertiary
                            } else {
                                theme.foreground
                            })
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(palette.tertiary)
                            .child(row.detail.clone()),
                    ),
            )
    });
    div()
        .absolute()
        .top_12()
        .right_3()
        .w_96()
        .p_1()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(palette.tertiary)
                .child("Sources · Space toggles · Enter applies · Esc cancels"),
        )
        .child(
            div()
                .id("source-menu-rows")
                .max_h_96()
                .overflow_y_scroll()
                .child(v_flex().children(rows)),
        )
}
