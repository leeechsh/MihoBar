//! The advanced window's connections page (zashboard's, in its table
//! mode): active or all connections, by source IP, searched with a regex,
//! some hidden by another; a table with chosen columns, sorted by a header,
//! grouped by a column; a connection's details; pause; close all.

use std::collections::HashSet;

use nsh_core::element::desktop::{MenuItem, popup_menu};
use nsh_core::element::{ImageSrc, input};
use nsh_core::prelude::*;

use crate::conns::{self, Column, Conn, Row};
use crate::icons;
use crate::model::Model;
use crate::style::z;

const ROW_HEIGHT: f32 = 36.0;

fn glyph(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> nsh_core::element::Image {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

/// A round icon button (`btn-ghost btn-circle btn-sm`).
pub(crate) fn icon_button(src: impl Fn() -> String + 'static, label: impl Fn() -> String + Clone + 'static, on_press: impl Fn() + 'static) -> impl IntoElement {
    let tip = label.clone();
    view()
        .w(px(32.0))
        .h(px(32.0))
        .items_center()
        .justify_center()
        .rounded(px(16.0))
        .hover(|s| s.bg(z::text(0.07)))
        .tooltip(crate::widgets::tip(move || tip()))
        .label(move || label())
        .on_press(on_press)
        .child(image(move || ImageSrc::from(src())).w(px(16.0)).h(px(16.0)).tint_color(z::text(0.8)))
}

/// A segmented control (zashboard's `SegmentedControl`): the chosen
/// option dark.
pub(crate) fn segment(label: impl Fn() -> String + 'static, on: impl Fn() -> bool + Clone + 'static, on_press: impl Fn() + 'static) -> impl IntoElement {
    let on2 = on.clone();
    view()
        .flex_row()
        .items_center()
        .h(px(28.0))
        .px(px(12.0))
        .rounded(px(7.0))
        .bg(move || if on() { z::primary(1.0) } else { z::primary(0.0) })
        .transition_colors()
        .duration(150.0)
        .on_press(on_press)
        .child(text(label).text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(move || if on2() { z::primary_content() } else { z::text(0.6) }))
}

pub(crate) fn segments(children: Vec<AnyElement>) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(px(10.0))
        .bg(z::base200(1.0))
        .border_1()
        .border_color(z::base300(0.6))
        .children(children)
}

/// A search field (`Search | Regex`), with a leading control and a clear
/// button.
pub(crate) fn search_field(placeholder: String, value: Signal<String>, leading: AnyElement, width: f32) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .w(px(width))
        .h(px(32.0))
        .px(px(8.0))
        .rounded(px(8.0))
        .bg(z::card())
        .border_1()
        .border_color(z::base300(0.7))
        .child(leading)
        .child(
            input()
                .flex_1()
                .value(move || value.get())
                .on_change(move |v: &str| value.set(v.to_owned()))
                .placeholder(placeholder)
                .text_size(px(13.0))
                .text_color(z::text(1.0)),
        )
        .child(when(
            move || !value.with(String::is_empty),
            move || view().on_press(move || value.set(String::new())).child(glyph(icons::z_xmark(), 12.0, || z::text(0.5))),
        ))
}

/// A select showing its value; a native menu of the options.
pub(crate) fn select_menu(label: impl Fn() -> String + 'static, width: f32, items: impl Fn() -> Vec<MenuItem> + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .w(px(width))
        .h(px(32.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .bg(z::card())
        .border_1()
        .border_color(z::base300(0.7))
        .on_press(move || {
            let _ = popup_menu(items());
        })
        .child(text(label).flex_1().truncate().text_size(px(13.0)).text_color(z::text(1.0)))
        .child(glyph(icons::z_chevron_down(), 11.0, || z::text(0.6)))
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Active,
    All,
}

#[derive(Clone, Copy)]
struct State {
    tab: Signal<Tab>,
    closed_only: Signal<bool>,
    search: Signal<String>,
    source_ip: Signal<Option<String>>,
    expanded: Signal<HashSet<String>>,
    details: Signal<Option<Conn>>,
    settings_open: Signal<bool>,
}

pub fn connections_page(m: Model) -> impl IntoElement {
    let state = State {
        tab: signal(Tab::Active),
        closed_only: signal(false),
        search: signal(String::new()),
        source_ip: signal(None),
        expanded: signal(HashSet::new()),
        details: signal(None),
        settings_open: signal(false),
    };
    let zh = m.is_zh();
    let rows_m = m.clone();
    let rendered = computed(move || {
        let m = &rows_m;
        let items: Vec<Conn> = match state.tab.get() {
            Tab::Active => m.z_active.with(|a| a.as_ref().clone()),
            Tab::All if state.closed_only.get() => m.z_closed.with(|c| c.as_ref().clone()),
            Tab::All => m.z_closed.with(|c| c.iter().cloned().chain(m.z_active.with(|a| a.as_ref().clone())).collect()),
        };
        let settings = m.z_settings.get();
        let search = state.search.get();
        let source = state.source_ip.get();
        let now_ms = crate::format::now_secs() * 1000;
        conns::render(
            &items,
            &conns::View {
                search: &search,
                search_hidden: settings.search_hidden_columns,
                hide: settings.hide_enabled.then_some(settings.hide_regex.as_str()),
                source_ip: source.as_deref(),
                columns: &settings.columns,
                sort: settings.sort,
                now_ms,
                zh,
            },
        )
    });
    let rows_m2 = m.clone();
    let rows = computed(move || {
        let list = rendered.get();
        match rows_m2.z_settings.with(|s| s.group_by) {
            Some(column) => state.expanded.with(|e| conns::grouped(list, column, e, crate::format::now_secs() * 1000, zh)),
            None => list.into_iter().map(Row::Conn).collect(),
        }
    });

    view()
        .flex_1()
        .pt(px(30.0))
        .child(controls(m.clone(), state, rendered))
        .child(table(m.clone(), state, rows, zh))
        .child(when(move || state.details.with(Option::is_some), {
            let m = m.clone();
            move || details_sheet(m.clone(), state)
        }))
        .child(when(move || state.settings_open.get(), move || settings_sheet(m.clone(), state)))
}

fn controls(m: Model, state: State, rendered: Memo<Vec<Conn>>) -> impl IntoElement {
    let count_m = m.clone();
    let active_label = move || {
        let all = count_m.z_active.with(|a| a.len());
        let shown = if state.tab.get() == Tab::Active { rendered.with(Vec::len) } else { all };
        if shown != all { format!("{} {shown} / {all}", count_m.tr("z.active")) } else { format!("{} {all}", count_m.tr("z.active")) }
    };
    let all_m = m.clone();
    let all_label = move || all_m.tr(if state.closed_only.get() { "z.closed" } else { "z.all" });
    let menu_m = m.clone();
    let tabs = segments(vec![
        segment(active_label, move || state.tab.get() == Tab::Active, move || state.tab.set(Tab::Active)).into_element(),
        segment(all_label, move || state.tab.get() == Tab::All, move || {
            if state.tab.get_untracked() == Tab::All {
                // Chosen again: all, or the closed ones only.
                let only = state.closed_only.get_untracked();
                let _ = popup_menu(vec![
                    MenuItem::new(menu_m.tr("z.all")).checked(!only).on_select(move || state.closed_only.set(false)),
                    MenuItem::new(menu_m.tr("z.closed")).checked(only).on_select(move || state.closed_only.set(true)),
                ]);
            }
            state.tab.set(Tab::All);
        })
        .into_element(),
    ]);
    let src_m = m.clone();
    let src_items_m = m.clone();
    let sources = select_menu(
        move || state.source_ip.get().unwrap_or_else(|| src_m.tr("z.all_sources")),
        140.0,
        move || {
            let mut ips: Vec<String> = src_items_m.z_active.with(|a| a.iter().map(|c| c.source_ip.clone()).collect::<HashSet<_>>().into_iter().collect());
            ips.sort();
            let current = state.source_ip.get_untracked();
            std::iter::once(MenuItem::new(src_items_m.tr("z.all_sources")).checked(current.is_none()).on_select(move || state.source_ip.set(None)))
                .chain(ips.into_iter().map(|ip| {
                    let on = current.as_deref() == Some(ip.as_str());
                    let value = ip.clone();
                    MenuItem::new(ip).checked(on).on_select(move || state.source_ip.set(Some(value.clone())))
                }))
                .collect()
        },
    );
    let funnel_m = m.clone();
    let funnel = view()
        .tooltip(crate::widgets::tip({
            let m = m.clone();
            move || m.tr(if m.z_settings.with(|s| s.search_hidden_columns) { "z.search_hidden_columns" } else { "z.search_visible_columns" })
        }))
        .on_press(move || funnel_m.z_settings.update(|s| s.search_hidden_columns = !s.search_hidden_columns))
        .child(image(ImageSrc::from(icons::z_funnel())).w(px(14.0)).h(px(14.0)).tint_color({
            let m = m.clone();
            move || if m.z_settings.with(|s| s.search_hidden_columns) { z::primary(1.0) } else { z::text(0.45) }
        }));
    let search = search_field(m.tr("z.search_regex"), state.search, funnel.into_element(), 260.0);

    let (s_m, hide_m, hide_l, pause_m, pause_l, close_m, close_l) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .pb(px(8.0))
        .child(tabs)
        .child(sources)
        .child(search)
        .child(view().flex_1())
        .child(icon_button(icons::z_tools, move || s_m.tr("z.connection_settings"), move || state.settings_open.set(true)))
        .child(icon_button(
            {
                let m = hide_m.clone();
                move || if m.z_settings.with(|s| s.hide_enabled) { icons::z_link_off() } else { icons::z_link() }
            },
            move || hide_l.tr(if hide_l.z_settings.with(|s| s.hide_enabled) { "z.show_connection" } else { "z.hide_connection" }),
            move || hide_m.z_settings.update(|s| s.hide_enabled = !s.hide_enabled),
        ))
        .child(icon_button(
            {
                let m = pause_m.clone();
                move || if m.z_paused.get() { icons::z_play() } else { icons::z_pause() }
            },
            move || pause_l.tr(if pause_l.z_paused.get() { "z.resume" } else { "z.pause" }),
            move || pause_m.z_paused.update(|p| *p = !*p),
        ))
        .child(icon_button(icons::z_xmark, move || close_l.tr("z.close_all"), move || {
            // zashboard: everything when the table lists all active ones,
            // else each one listed.
            let targets: Vec<String> = rendered.with_untracked(|r| r.iter().filter(|c| !c.closed).map(|c| c.id.clone()).collect());
            if targets.is_empty() {
                return;
            }
            let m = close_m.clone();
            let all = targets.len() == m.z_active.with_untracked(|a| a.len());
            crate::model::run_root(async move {
                if all {
                    m.close_all_connections().await;
                } else {
                    for id in targets {
                        m.close_connection(id).await;
                    }
                }
            });
        }))
}

/// A cell's box: its natural width, and its share of any width left.
fn cell_box(column: Column) -> View {
    view()
        .flex_row()
        .items_center()
        .px(px(10.0))
        .basis(px(column.width()))
        .min_w(px(column.width()))
        .flex_grow(column.grow())
        .flex_shrink(0.0)
        .overflow_hidden()
}

/// The table's width: its columns', at least the window's.
fn table_width(columns: &[Column], viewport: f32) -> f32 {
    columns.iter().map(|c| c.width()).sum::<f32>().max(viewport)
}

fn table(m: Model, state: State, rows: Memo<Vec<Row>>, zh: bool) -> impl IntoElement {
    let head_m = m.clone();
    let header = dynamic(move || {
        let columns = head_m.z_settings.with(|s| s.columns.clone());
        let cells: Vec<_> = columns.into_iter().map(|c| header_cell(head_m.clone(), c)).collect();
        view()
            .flex_row()
            .items_center()
            .h(px(40.0))
            .border_b_1()
            .border_color(z::base300(0.6))
            .children(cells)
    });
    let list_m = m.clone();
    let list = virtual_list(move || rows.with(Vec::len), move |at: nsh_core::element::Position| {
        let item = computed(move || rows.with(|r| r.get(at.index()).cloned()));
        let m = list_m.clone();
        view().child(dynamic(move || match item.get() {
            Some(Row::Group { value, count, expanded }) => group_row(state, value, count, expanded).into_element(),
            Some(Row::Conn(c)) => conn_row(m.clone(), state, c, at.index(), zh).into_element(),
            None => view().into_element(),
        }))
    })
    .estimated_item_size(px(ROW_HEIGHT))
    .flex_1();
    let viewport = signal(1000.0_f32);
    let width_m = m.clone();
    view()
        .flex_1()
        .mx(px(12.0))
        .mb(px(12.0))
        .bg(z::card())
        .rounded(px(12.0))
        .border_1()
        .border_color(z::base300(0.35))
        .shadow_xs()
        .overflow_hidden()
        .on_layout(move |b: &Bounds<Points>| viewport.set(b.size.width.0))
        .child(
            scroll().horizontal().flex_1().scrollbar(false).child(
                view()
                    .h_full()
                    .w(move || px(width_m.z_settings.with(|s| table_width(&s.columns, viewport.get()))))
                    .child(header)
                    .child(list),
            ),
        )
}

fn header_cell(m: Model, column: Column) -> impl IntoElement {
    let sorted = {
        let m = m.clone();
        move || m.z_settings.with(|s| s.sort.filter(|(c, _)| *c == column).map(|(_, desc)| desc))
    };
    let grouped = {
        let m = m.clone();
        move || m.z_settings.with(|s| s.group_by == Some(column))
    };
    let press_m = m.clone();
    let group_m = m.clone();
    let mut cell = cell_box(column)
        .h_full()
        .gap(px(4.0))
        .child(text(m.tr(column.title_key())).min_w(px(0.0)).flex_shrink(1.0).truncate().text_size(px(13.0)).text_color(z::text(0.6)))
        .child(dynamic(move || match sorted() {
            Some(false) => glyph(icons::z_sort_up(), 14.0, || z::text(0.6)).into_element(),
            Some(true) => glyph(icons::z_sort_down(), 14.0, || z::text(0.6)).into_element(),
            None => view().into_element(),
        }));
    if column.groupable() {
        cell = cell.child(
            view()
                .w(px(20.0))
                .h(px(20.0))
                .items_center()
                .justify_center()
                .rounded(px(10.0))
                .hover(|s| s.bg(z::text(0.08)))
                .on_press(move || group_m.z_settings.update(|s| s.group_by = if s.group_by == Some(column) { None } else { Some(column) }))
                .child(image(move || ImageSrc::from(if grouped() { icons::z_zoom_out() } else { icons::z_zoom_in() })).w(px(13.0)).h(px(13.0)).tint_color(z::text(0.55))),
        );
    }
    if column.sortable() {
        // None → first order (largest first for numbers) → the other → none.
        cell = cell.on_press(move || {
            press_m.z_settings.update(|s| {
                let first = column.numeric();
                s.sort = match s.sort {
                    Some((c, desc)) if c == column && desc == first => Some((column, !first)),
                    Some((c, _)) if c == column => None,
                    _ => Some((column, first)),
                };
            })
        });
    }
    cell
}

fn group_row(state: State, value: String, count: usize, expanded: bool) -> impl IntoElement {
    let key = value.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(ROW_HEIGHT))
        .px(px(12.0))
        .bg(z::base200(0.6))
        .border_b_1()
        .border_color(z::base300(0.35))
        .hover(|s| s.bg(z::base200(1.0)))
        .on_press(move || {
            state.expanded.update(|e| {
                if !e.remove(&key) {
                    e.insert(key.clone());
                }
            })
        })
        .child(glyph(if expanded { icons::z_chevron_down() } else { icons::z_chevron_right() }, 12.0, || z::text(0.7)))
        .child(text(if value.is_empty() { "-".to_owned() } else { value }).truncate().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(1.0)))
        .child(text(format!("({count})")).text_size(px(13.0)).text_color(z::text(0.5)))
}

fn conn_row(m: Model, state: State, c: Conn, index: usize, zh: bool) -> impl IntoElement {
    let columns = m.z_settings.with_untracked(|s| s.columns.clone());
    let now_ms = crate::format::now_secs() * 1000;
    let tone = if c.closed { 0.45 } else { 1.0 };
    let cells: Vec<AnyElement> = columns
        .into_iter()
        .map(|column| match column {
            Column::Close => {
                let id = c.id.clone();
                let m = m.clone();
                let closed = c.closed;
                cell_box(column)
                    .justify_center()
                    .child(when(move || !closed, move || {
                        let id = id.clone();
                        let m = m.clone();
                        view()
                            .w(px(24.0))
                            .h(px(24.0))
                            .items_center()
                            .justify_center()
                            .rounded(px(12.0))
                            .hover(|s| s.bg(z::text(0.08)))
                            .on_press(move || {
                                let m = m.clone();
                                let id = id.clone();
                                crate::model::run_root(async move { m.close_connection(id).await });
                            })
                            .child(glyph(icons::z_xmark(), 13.0, || z::text(0.6)))
                    }))
                    .into_element()
            }
            Column::Chains => {
                let parts: Vec<String> = c.chains.iter().rev().cloned().collect();
                let n = parts.len();
                let mut row = cell_box(column).gap(px(4.0));
                for (i, part) in parts.into_iter().enumerate() {
                    row = row.child(text(part).min_w(px(0.0)).flex_shrink(1.0).truncate().text_size(px(13.0)).text_color(z::text(tone)));
                    if i + 1 < n {
                        row = row.child(glyph(icons::z_chain_arrow(), 12.0, || z::text(0.5)));
                    }
                }
                row.into_element()
            }
            _ => cell_box(column)
                .child(text(column.text(&c, now_ms, zh)).flex_1().min_w(px(0.0)).truncate().text_size(px(13.0)).text_color(z::text(tone)))
                .into_element(),
        })
        .collect();
    let detail = c.clone();
    view()
        .flex_row()
        .items_center()
        .h(px(ROW_HEIGHT))
        .bg(if index % 2 == 1 { z::base200(0.35) } else { z::card() })
        .hover(|s| s.bg(z::base200(1.0)))
        .on_press(move || state.details.set(Some(detail.clone())))
        .children(cells)
}

/// A sheet over the page: a dimmed backdrop (pressing it closes) and a
/// card.
pub(crate) fn sheet(title: String, width: f32, height: f32, on_close: impl Fn() + Clone + 'static, actions: Vec<AnyElement>, body: impl IntoElement) -> impl IntoElement {
    let close = on_close.clone();
    view()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .items_center()
        .justify_center()
        .child(view().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).bottom(px(0.0)).bg(z::text(0.25)).on_press(move || close()))
        .child(
            view()
                .w(px(width))
                .h(px(height))
                .bg(z::card())
                .rounded(px(16.0))
                .shadow_lg()
                .overflow_hidden()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(20.0))
                        .py(px(14.0))
                        .border_b_1()
                        .border_color(z::base300(0.6))
                        .child(text(title).flex_1().text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(1.0)))
                        .children(actions)
                        .child(icon_button(icons::z_xmark, || String::new(), move || on_close())),
                )
                .child(scroll().flex_1().child(body)),
        )
}

fn details_sheet(m: Model, state: State) -> impl IntoElement {
    let Some(c) = state.details.get_untracked() else { return view().into_element() };
    let json = serde_json::to_string_pretty(c.raw.0.as_ref()).unwrap_or_default();
    let copy = json.clone();
    let id = c.id.clone();
    let close_m = m.clone();
    let mut actions = vec![icon_button(icons::z_copy, {
        let m = m.clone();
        move || m.tr("z.copy")
    }, move || set_clipboard(copy.clone()))
    .into_element()];
    if !c.closed {
        actions.push(
            icon_button(icons::z_link_off, {
                let m = m.clone();
                move || m.tr("z.close_connection")
            }, move || {
                let m = close_m.clone();
                let id = id.clone();
                state.details.set(None);
                crate::model::run_root(async move { m.close_connection(id).await });
            })
            .into_element(),
        );
    }
    sheet(
        m.tr("z.connection_details"),
        640.0,
        600.0,
        move || state.details.set(None),
        actions,
        text(json).p(px(20.0)).font_family("Menlo").text_size(px(12.0)).text_color(z::text(0.9)),
    )
    .into_element()
}

fn setting_row(label: String, control: impl IntoElement) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .min_h(px(40.0))
        .child(text(label).flex_1().text_size(px(13.0)).text_color(z::text(1.0)))
        .child(control)
}

fn settings_sheet(m: Model, state: State) -> impl IntoElement {
    let group_m = m.clone();
    let group_items_m = m.clone();
    let group_by = select_menu(
        move || group_m.z_settings.with(|s| s.group_by).map_or_else(|| group_m.tr("z.no_grouping"), |c| group_m.tr(c.title_key())),
        180.0,
        move || {
            let m = group_items_m.clone();
            let current = m.z_settings.with_untracked(|s| s.group_by);
            let none_m = m.clone();
            std::iter::once(MenuItem::new(m.tr("z.no_grouping")).checked(current.is_none()).on_select(move || none_m.z_settings.update(|s| s.group_by = None)))
                .chain(Column::ALL.into_iter().filter(|c| c.groupable()).map(|c| {
                    let m2 = m.clone();
                    MenuItem::new(m.tr(c.title_key())).checked(current == Some(c)).on_select(move || m2.z_settings.update(|s| s.group_by = Some(c)))
                }))
                .collect()
        },
    );
    let regex = signal(m.z_settings.with_untracked(|s| s.hide_regex.clone()));
    {
        let m = m.clone();
        effect(move || {
            let r = regex.get();
            untracked(|| m.z_settings.update(|s| s.hide_regex = r));
        });
    }
    let hide_m = m.clone();
    let hide_toggle = view()
        .on_press(move || hide_m.z_settings.update(|s| s.hide_enabled = !s.hide_enabled))
        .child(crate::widgets::switch({
            let m = m.clone();
            move || m.z_settings.with(|s| s.hide_enabled)
        }, || false));
    let hidden_m = m.clone();
    let hidden_toggle = view()
        .on_press(move || hidden_m.z_settings.update(|s| s.search_hidden_columns = !s.search_hidden_columns))
        .child(crate::widgets::switch({
            let m = m.clone();
            move || m.z_settings.with(|s| s.search_hidden_columns)
        }, || false));
    let columns_m = m.clone();
    let column_list = dynamic(move || {
        let m = columns_m.clone();
        let shown = m.z_settings.with(|s| s.columns.clone());
        let hidden: Vec<Column> = Column::ALL.into_iter().filter(|c| !shown.contains(c)).collect();
        let rows: Vec<AnyElement> = shown
            .iter()
            .copied()
            .map(|c| column_setting_row(m.clone(), c, true))
            .chain(hidden.into_iter().map(|c| column_setting_row(m.clone(), c, false)))
            .collect();
        view().gap(px(2.0)).children(rows)
    });
    sheet(
        m.tr("z.connection_settings"),
        520.0,
        620.0,
        move || state.settings_open.set(false),
        Vec::new(),
        view()
            .px(px(20.0))
            .py(px(12.0))
            .gap(px(4.0))
            .child(setting_row(m.tr("z.group_by"), group_by))
            .child(setting_row(m.tr("z.hide_connection_regex"), search_field(String::new(), regex, view().into_element(), 220.0)))
            .child(setting_row(m.tr("z.hide_connection"), hide_toggle))
            .child(setting_row(m.tr("z.search_hidden_columns"), hidden_toggle))
            .child(text(m.tr("z.table_columns")).mt(px(12.0)).mb(px(4.0)).text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(0.6)))
            .child(column_list),
    )
}

/// A column in the settings: shown or not, and moved up or down.
fn column_setting_row(m: Model, column: Column, shown: bool) -> AnyElement {
    let (toggle_m, up_m, down_m) = (m.clone(), m.clone(), m.clone());
    let mv = |m: Model, delta: isize| {
        move || {
            m.z_settings.update(|s| {
                if let Some(i) = s.columns.iter().position(|c| *c == column) {
                    let j = i as isize + delta;
                    if j >= 0 && (j as usize) < s.columns.len() {
                        s.columns.swap(i, j as usize);
                    }
                }
            })
        }
    };
    let mut row = view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .h(px(34.0))
        .px(px(8.0))
        .rounded(px(8.0))
        .hover(|s| s.bg(z::base200(1.0)))
        .child(
            view()
                .w(px(18.0))
                .h(px(18.0))
                .items_center()
                .justify_center()
                .rounded(px(5.0))
                .border_1()
                .border_color(z::base300(1.0))
                .bg(if shown { z::primary(1.0) } else { z::card() })
                .on_press(move || {
                    toggle_m.z_settings.update(|s| {
                        if let Some(i) = s.columns.iter().position(|c| *c == column) {
                            s.columns.remove(i);
                        } else {
                            s.columns.push(column);
                        }
                    })
                })
                .children(shown.then(|| glyph(icons::z_check(), 12.0, z::primary_content))),
        )
        .child(text(m.tr(column.title_key())).flex_1().text_size(px(13.0)).text_color(z::text(if shown { 1.0 } else { 0.5 })));
    if shown {
        row = row
            .child(icon_button(icons::z_up, || String::new(), mv(up_m, -1)))
            .child(icon_button(icons::z_down, || String::new(), mv(down_m, 1)));
    }
    row.into_element()
}
