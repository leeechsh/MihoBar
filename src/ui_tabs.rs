//! The connections, logs and rules tabs (ClashBar's `ConnectionsTabView`,
//! `LogsTabView`, `RulesTabView` and their common controls).

use std::rc::Rc;

use nsh_core::element::ImageSrc;
use nsh_core::element::desktop::{MenuItem, popup_menu, popup_menu_at};
use nsh_core::element::input;
use nsh_core::prelude::*;

use crate::data::{self, Connection, ConnectionSort, RuleItem, RuleTypeFilter, TransportFilter};
use crate::format::{self, Relative};
use crate::icons;
use crate::model::{LogEntry, LogSource, Model};
use crate::style::{self, font};
use crate::widgets;

fn icon_view(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

// --- Common controls ----------------------------------------------------------

/// ClashBar's `tabControlCard`: the filters above a list.
fn control_card() -> View {
    widgets::card().gap(px(style::SPACE6)).p(px(style::SPACE6))
}

/// A search field (ClashBar's `filterTextField`).
fn filter_field(placeholder: impl Fn() -> String + 'static, value: Signal<String>) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE4))
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(7.0))
        .bg(style::control_track())
        .child(icon_view(icons::magnifier(), 11.0, style::secondary_label))
        .child(
            input()
                .flex_1()
                .value(move || value.get())
                .on_change(move |v: &str| value.set(v.to_owned()))
                .placeholder(placeholder())
                .text_size(px(font::CALLOUT))
                .text_color(style::primary_label()),
        )
        .child(when(
            move || !value.with(|v| v.is_empty()),
            move || {
                view()
                    .on_press(move || value.set(String::new()))
                    .child(icon_view(icons::xmark(), 9.0, style::tertiary_label))
            },
        ))
}

/// A small round icon button with a tooltip.
fn icon_button(
    src: impl Fn() -> String + 'static,
    tone: impl Fn() -> AdaptiveColor + 'static,
    label: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> View {
    widgets::icon_button(src, tone, label, on_press)
}

/// A selection menu behind an icon (ClashBar's `compactSelectionMenu`).
fn selection_menu<T: Copy + PartialEq + 'static>(
    m: Model,
    icon: String,
    help: &'static str,
    options: &'static [T],
    current: impl Fn() -> T + 'static,
    title: fn(T) -> &'static str,
    on_select: impl Fn(T) + Clone + 'static,
) -> View {
    let current = Rc::new(current);
    let label_m = m.clone();
    icon_button(move || icon.clone(), style::secondary_label, move || label_m.tr(help), move || {
        let selected = current();
        let items = options
            .iter()
            .map(|&option| {
                let on_select = on_select.clone();
                MenuItem::new(m.tr(title(option))).checked(option == selected).on_select(move || on_select(option))
            })
            .collect();
        let _ = popup_menu(items);
    })
}

fn separator() -> impl IntoElement {
    view().h(px(0.65)).bg(style::separator())
}

fn badge(content: impl Fn() -> String + 'static) -> impl IntoElement {
    text(content)
        .text_size(px(font::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(style::secondary_label())
        .px(px(style::SPACE6))
        .py(px(1.0))
        .rounded(px(8.0))
        .bg(style::control_track())
}

// --- Connections ----------------------------------------------------------------

pub fn connections_tab(m: Model) -> impl IntoElement {
    let filter_text = signal(String::new());
    let visible = {
        let m = m.clone();
        computed(move || {
            let (transport, sort) = m.settings.with(|s| (s.connections_transport_filter, s.connections_sort));
            m.connections.with(|all| data::visible_connections(all, &filter_text.get(), transport, sort))
        })
    };
    let (filter_m, sort_m, close_m, badge_m, place_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    let controls = control_card()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE6))
                .child(selection_menu(
                    m.clone(),
                    icons::filter_lines(),
                    "ui.network.filter.transport",
                    &TransportFilter::ALL,
                    move || filter_m.settings.with(|s| s.connections_transport_filter),
                    TransportFilter::title_key,
                    {
                        let m = m.clone();
                        move |f| m.settings.update(|s| s.connections_transport_filter = f)
                    },
                ))
                .child(selection_menu(
                    m.clone(),
                    icons::arrow_up_arrow_down(),
                    "ui.network.sort.label",
                    &ConnectionSort::ALL,
                    move || sort_m.settings.with(|s| s.connections_sort),
                    ConnectionSort::title_key,
                    {
                        let m = m.clone();
                        move |o| m.settings.update(|s| s.connections_sort = o)
                    },
                ))
                .child(view().flex_1())
                .child(badge(move || {
                    let total = badge_m.connections.with(|c| c.len());
                    format!("{}/{total}", visible.with(|v| v.len()))
                }))
                .child(icon_button(icons::xmark, style::negative, {
                    let m = m.clone();
                    move || m.tr("ui.action.close_all")
                }, move || {
                    let m = close_m.clone();
                    crate::model::run_root(async move { m.close_all_connections().await });
                })),
        )
        .child(filter_field(move || place_m.tr("ui.placeholder.filter_connection"), filter_text));
    let empty_m = m.clone();
    let rows_m = m.clone();
    view()
        .flex_1()
        .gap(px(style::SPACE6))
        .child(controls)
        .child(when(
            move || visible.with(|v| v.is_empty()),
            move || crate::ui_proxy::empty_card(empty_m.tr("ui.empty.connections")),
        ))
        .child(virtual_rows(visible, 52.0, move |conn| connection_row(rows_m.clone(), conn)))
}

/// `rows` as a list that draws only the rows on screen (the management
/// window's lists): each row reads its item by position.
fn virtual_rows<T, R>(rows: Memo<Vec<T>>, estimated: f32, row: impl Fn(ReadSignal<T>) -> R + Clone + 'static) -> impl IntoElement
where
    T: Clone + PartialEq + 'static,
    R: IntoElement + 'static,
{
    virtual_list(move || rows.with(|r| r.len()), move |at: nsh_core::element::Position| {
        let item = computed(move || rows.with(|r| r.get(at.index()).cloned()));
        let row = row.clone();
        view().child(when(
            move || item.with(|i| i.is_some()),
            move || {
                let Some(first) = item.get_untracked() else { return view() };
                let current = signal(first);
                effect(move || {
                    if let Some(next) = item.get() {
                        if current.with_untracked(|c| *c != next) {
                            current.set(next);
                        }
                    }
                });
                view().child(row(current.read_only()))
            },
        ))
    })
    .estimated_item_size(px(estimated))
    .separator(separator)
    .flex_1()
}

/// The row's icon and its color (ClashBar's `connectionVisual`).
fn connection_visual(c: &Connection) -> (String, AdaptiveColor) {
    let host = c.metadata.host.as_deref().unwrap_or("").to_lowercase();
    let network = c.metadata.network.as_deref().unwrap_or("").to_lowercase();
    if host.contains("google") || host.contains("gstatic") {
        (icons::shield(), style::purple())
    } else if host.contains("icloud") || host.contains("apple") {
        (icons::icloud(), style::info())
    } else if host.contains("github") {
        (icons::terminal(), style::indigo())
    } else if host.contains("twitter") || host.contains("x.com") {
        (icons::lock(), style::positive())
    } else if host.contains("amazon") {
        (icons::cart(), style::warning())
    } else if network.contains("udp") {
        (icons::radiowaves(), style::teal())
    } else if network.contains("tcp") {
        (icons::network(), style::info())
    } else {
        (icons::globe(), style::secondary_label())
    }
}

fn metric(icon: String, value: impl Fn() -> String + 'static, color: fn() -> AdaptiveColor) -> impl IntoElement {
    view()
        .flex_1()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE2))
        .child(icon_view(icon, 9.0, color))
        .child(text(value).truncate().text_size(px(font::CAPTION)).text_color(color()))
}

fn connection_row(m: Model, conn: ReadSignal<Connection>) -> impl IntoElement {
    let na = {
        let m = m.clone();
        move || m.tr("ui.common.na")
    };
    let menu_m = m.clone();
    let close_m = m.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .px(px(style::SPACE4))
        .py(px(style::SPACE2))
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.05)))
        .on_context_menu(move |e: &ContextMenuEvent| {
            let c = conn.get_untracked();
            let mut items = Vec::new();
            {
                let m = menu_m.clone();
                let id = c.id.clone();
                items.push(MenuItem::new(menu_m.tr("ui.action.close_connection")).destructive(true).on_select(move || {
                    let m = m.clone();
                    let id = id.clone();
                    crate::model::run_root(async move { m.close_connection(id).await });
                }));
            }
            if let Some(host) = c.resolved_host() {
                let m = menu_m.clone();
                items.push(MenuItem::new(menu_m.tr("ui.action.copy_host")).on_select(move || m.copy_connection_host(&host)));
            }
            {
                let m = menu_m.clone();
                let id = c.id.clone();
                items.push(MenuItem::new(menu_m.tr("ui.action.copy_connection_id")).on_select(move || m.copy_connection_id(&id)));
            }
            let _ = popup_menu_at(items, e.page_position);
        })
        .child(image(move || ImageSrc::from(connection_visual(&conn.get()).0)).w(px(14.0)).h(px(14.0)).tint_color(move || connection_visual(&conn.get()).1))
        .child(
            view()
                .flex_1()
                .gap(px(style::SPACE2))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(style::SPACE2))
                        .h(px(16.0))
                        .child(
                            text({
                                let na = na.clone();
                                move || conn.with(|c| c.resolved_host()).unwrap_or_else(&na)
                            })
                            .flex_1()
                            .truncate()
                            .text_size(px(font::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(style::primary_label()),
                        )
                        .child(
                            text(move || {
                                conn.with(|c| {
                                    let parsed = data::parse_rule(c.rule.as_deref());
                                    data::rule_type_text(c.rule.as_deref(), parsed.as_ref().map(|p| p.0.as_str()))
                                })
                            })
                            .text_size(px(font::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(style::secondary_label())
                            .px(px(style::SPACE4))
                            .rounded(px(5.0))
                            .bg(style::badge_fill()),
                        )
                        .child(
                            text(move || {
                                conn.with(|c| {
                                    c.rule_payload
                                        .clone()
                                        .or_else(|| data::parse_rule(c.rule.as_deref()).and_then(|p| p.1))
                                        .unwrap_or_else(|| "--".into())
                                })
                            })
                            .max_w(px(110.0))
                            .truncate()
                            .text_size(px(font::CAPTION))
                            .text_color(style::tertiary_label()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(style::SPACE2))
                        .h(px(16.0))
                        .child(metric(icons::clock(), move || data::time_only(conn.with(|c| c.start.clone()).as_deref()), style::secondary_label))
                        .child(metric(
                            icons::network(),
                            move || conn.with(|c| c.metadata.network.clone()).map(|n| n.to_uppercase()).unwrap_or_else(|| "--".into()),
                            style::secondary_label,
                        ))
                        .child(metric(icons::arrow_up(), move || format::bytes_compact_no_space(conn.with(|c| c.upload)), style::info))
                        .child(metric(icons::arrow_down(), move || format::bytes_compact_no_space(conn.with(|c| c.download)), style::positive)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(style::SPACE2))
                        .child(icon_view(icons::nodes_graph(), 9.0, style::tertiary_label))
                        .child(
                            text(move || {
                                let parts = conn.with(|c| c.chain_parts());
                                if parts.is_empty() { na() } else { parts.join(" > ") }
                            })
                            .flex_1()
                            .truncate()
                            .text_size(px(font::CAPTION))
                            .text_color(style::secondary_label()),
                        ),
                ),
        )
        .child(
            view()
                .w(px(14.0))
                .h(px(14.0))
                .items_center()
                .justify_center()
                .tooltip(crate::widgets::tip({
                    let m = m.clone();
                    move || m.tr("ui.action.close_connection")
                }))
                .on_press(move || {
                    let m = close_m.clone();
                    let id = conn.get_untracked().id;
                    crate::model::run_root(async move { m.close_connection(id).await });
                })
                .child(icon_view(icons::xmark(), 9.0, style::tertiary_label)),
        )
}

// --- Logs ------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum LevelFilter {
    Info,
    Warning,
    Error,
}

fn level_filter(normalized: &str) -> LevelFilter {
    match normalized {
        "ERROR" => LevelFilter::Error,
        "WARNING" => LevelFilter::Warning,
        _ => LevelFilter::Info,
    }
}

fn level_presentation(f: LevelFilter) -> (String, AdaptiveColor) {
    match f {
        LevelFilter::Error => (icons::octagon_fill(), style::negative()),
        LevelFilter::Warning => (icons::warning_triangle(), style::warning()),
        LevelFilter::Info => (icons::info_circle_fill(), style::info()),
    }
}

/// A row of toggle chips, "all" first (ClashBar's `logFilterGroup`).
fn chips<T: Copy + Eq + std::hash::Hash + 'static>(
    m: Model,
    icon: String,
    all_key: &'static str,
    items: Vec<(T, &'static str)>,
    selection: Signal<std::collections::HashSet<T>>,
) -> impl IntoElement {
    let chip = |label: Rc<dyn Fn() -> String>, on: Rc<dyn Fn() -> bool>, press: Rc<dyn Fn()>| {
        let on2 = on.clone();
        text(move || label())
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || if on() { style::info() } else { style::secondary_label() })
            .px(px(style::SPACE6))
            .py(px(1.0))
            .rounded(px(8.0))
            .bg(move || if on2() { style::accent(0.16) } else { style::fill(0.05) })
            .on_press(move || press())
    };
    let all = {
        let m = m.clone();
        chip(
            Rc::new(move || m.tr(all_key)),
            Rc::new(move || selection.with(|s| s.is_empty())),
            Rc::new(move || selection.set(Default::default())),
        )
    };
    let rest: Vec<_> = items
        .into_iter()
        .map(|(value, key)| {
            let m = m.clone();
            chip(
                Rc::new(move || m.tr(key)),
                Rc::new(move || selection.with(|s| s.contains(&value))),
                Rc::new(move || {
                    selection.update(|s| {
                        if !s.remove(&value) {
                            s.insert(value);
                        }
                    })
                }),
            )
        })
        .collect();
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE4))
        .child(icon_view(icon, 10.0, style::tertiary_label))
        .child(all)
        .children(rest)
}

pub fn logs_tab(m: Model) -> impl IntoElement {
    let search = signal(String::new());
    let sources = signal(std::collections::HashSet::<LogSource>::new());
    let levels = signal(std::collections::HashSet::<LevelFilter>::new());
    let visible = {
        let m = m.clone();
        computed(move || {
            let keyword = search.get().trim().to_lowercase();
            let (sources, levels) = (sources.get(), levels.get());
            m.logs.with(|logs| {
                logs.iter()
                    .filter(|e| sources.is_empty() || sources.contains(&e.source))
                    .filter(|e| levels.is_empty() || levels.contains(&level_filter(data::normalized_level(&e.level))))
                    .filter(|e| {
                        keyword.is_empty()
                            || format!(
                                "{} {} {} {}",
                                source_label(&m, e.source),
                                data::normalized_level(&e.level),
                                data::date_time(e.at),
                                e.message
                            )
                            .to_lowercase()
                            .contains(&keyword)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
        })
    };
    let level_m = m.clone();
    let level_menu = view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE2))
        .px(px(style::SPACE6))
        .py(px(style::SPACE2))
        .rounded(px(9.0))
        .bg(style::badge_fill())
        .on_press(move || {
            let current = level_m.log_level.get_untracked();
            let items = data::LOG_LEVELS
                .iter()
                .map(|&level| {
                    let m = level_m.clone();
                    MenuItem::new(level).checked(current.eq_ignore_ascii_case(level)).on_select(move || {
                        let m = m.clone();
                        crate::model::run_root(async move { m.set_core_log_level(level).await });
                    })
                })
                .collect();
            let _ = popup_menu(items);
        })
        .child(icon_view(icons::text_alignleft(), 10.0, style::secondary_label))
        .child(
            text({
                let m = m.clone();
                move || {
                    let l = m.log_level.get();
                    if data::LOG_LEVELS.contains(&l.as_str()) { l } else { "info".into() }
                }
            })
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::MEDIUM)
            .text_color(style::secondary_label()),
        )
        .child(icon_view(icons::chevron_down(), 7.0, style::tertiary_label));
    let (copy_m, clear_m, place_m) = (m.clone(), m.clone(), m.clone());
    let controls = control_card()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE6))
                .child(chips(
                    m.clone(),
                    icons::doc_text(),
                    "ui.log_source.all",
                    vec![(LogSource::App, "ui.log_source.clashbar"), (LogSource::Mihomo, "ui.log_source.mihomo")],
                    sources,
                ))
                .child(view().flex_1())
                .child(level_menu),
        )
        .child(filter_field(move || place_m.tr("ui.placeholder.search_logs"), search))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE6))
                .child(chips(
                    m.clone(),
                    icons::filter_lines(),
                    "ui.log_filter.all",
                    vec![
                        (LevelFilter::Info, "ui.log_filter.info"),
                        (LevelFilter::Warning, "ui.log_filter.warning"),
                        (LevelFilter::Error, "ui.log_filter.error"),
                    ],
                    levels,
                ))
                .child(view().flex_1())
                .child(icon_button(icons::doc_on_doc, style::secondary_label, {
                    let m = m.clone();
                    move || m.tr("ui.action.copy_all_logs")
                }, move || copy_m.copy_all_logs()))
                .child(icon_button(icons::trash, style::negative, {
                    let m = m.clone();
                    move || m.tr("ui.action.clear_all_logs")
                }, move || clear_m.clear_all_logs())),
        );
    let empty_m = m.clone();
    let rows_m = m.clone();
    view()
        .flex_1()
        .gap(px(style::SPACE6))
        .child(controls)
        .child(when(
            move || visible.with(|v| v.is_empty()),
            move || crate::ui_proxy::empty_card(empty_m.tr("ui.empty.logs")),
        ))
        .child(virtual_rows(visible, 44.0, move |entry| log_row(rows_m.clone(), entry)))
}

fn source_label(m: &Model, source: LogSource) -> String {
    m.tr(match source {
        LogSource::App => "ui.log_source.clashbar",
        LogSource::Mihomo => "ui.log_source.mihomo",
    })
}

fn log_row(m: Model, entry: ReadSignal<LogEntry>) -> impl IntoElement {
    let parsed = move || entry.with(|e| data::parse_log_message(&e.message));
    let level = move || level_filter(data::normalized_level(&entry.with(|e| e.level.clone())));
    let dot = || text("•").text_size(px(font::CAPTION)).text_color(style::tertiary_label());
    let menu_m = m.clone();
    let tag_shown = {
        let parsed = parsed.clone();
        move || parsed().protocol_tag.is_some()
    };
    let detail_shown = {
        let parsed = parsed.clone();
        move || parsed().detail_text.is_some()
    };
    let (tag_parsed, detail_parsed, main_parsed) = (parsed.clone(), parsed.clone(), parsed.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .px(px(style::SPACE4))
        .py(px(style::SPACE4))
        .on_context_menu(move |e: &ContextMenuEvent| {
            let log = entry.get_untracked();
            let (a, b) = (menu_m.clone(), menu_m.clone());
            let (l1, l2) = (log.clone(), log);
            let items = vec![
                MenuItem::new(menu_m.tr("ui.action.copy_log_message")).on_select(move || a.copy_log_message(&l1)),
                MenuItem::new(menu_m.tr("ui.action.copy_log_entry")).on_select(move || b.copy_log_entry(&l2)),
            ];
            let _ = popup_menu_at(items, e.page_position);
        })
        // The level as a small dot (the tinted glyphs lose their marks).
        .child(
            view()
                .w(px(7.0))
                .h(px(7.0))
                .mt(px(5.0))
                .rounded(px(3.5))
                .bg(move || level_presentation(level()).1),
        )
        .child(
            view()
                .flex_1()
                .gap(px(style::SPACE2))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(style::SPACE2))
                        .child(
                            text({
                                let m = m.clone();
                                move || source_label(&m, entry.with(|e| e.source))
                            })
                            .text_size(px(font::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || match entry.with(|e| e.source) {
                                LogSource::App => style::secondary_label(),
                                LogSource::Mihomo => style::info(),
                            }),
                        )
                        .child(when(tag_shown, move || {
                            let tag_parsed = tag_parsed.clone();
                            let color_parsed = tag_parsed.clone();
                            view().flex_row().gap(px(style::SPACE2)).child(dot()).child(
                                text(move || tag_parsed().protocol_tag.unwrap_or_default())
                                    .text_size(px(font::CAPTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || {
                                        let tag = color_parsed().protocol_tag.unwrap_or_default();
                                        if tag.contains("UDP") {
                                            style::warning()
                                        } else if tag.contains("DNS") {
                                            style::positive()
                                        } else {
                                            style::info()
                                        }
                                    }),
                            )
                        }))
                        .child(dot())
                        .child(
                            text(move || data::date_time(entry.with(|e| e.at)))
                                .text_size(px(font::CAPTION))
                                .text_color(style::tertiary_label()),
                        ),
                )
                .child(
                    text({
                        let m = m.clone();
                        move || {
                            let t = main_parsed().main_text;
                            if t.is_empty() { m.tr("ui.common.na") } else { t }
                        }
                    })
                    .text_size(px(font::CAPTION))
                    .text_color(style::primary_label()),
                )
                .child(when(detail_shown, move || {
                    let detail_parsed = detail_parsed.clone();
                    view()
                        .flex_row()
                        .gap(px(style::SPACE6))
                        .child(view().w(px(1.0)).bg(move || level_presentation(level()).1.opacity(0.18)))
                        .child(
                            text(move || detail_parsed().detail_text.unwrap_or_default())
                                .flex_1()
                                .text_size(px(font::CAPTION))
                                .text_color(style::secondary_label()),
                        )
                })),
        )
}

// --- Rules -------------------------------------------------------------------------

/// A row of the rules list: a policy group's header, or a rule.
#[derive(Clone, PartialEq)]
enum RuleRow {
    Header { policy: String, count: usize, expanded: bool },
    Rule(RuleItem),
}

fn rule_type_icon(kind: &str) -> (String, AdaptiveColor) {
    let lower = kind.to_lowercase();
    if lower.contains("ipcidr") {
        (icons::globe_fill(), style::info())
    } else if lower.contains("domain") || lower.contains("suffix") || lower.contains("keyword") {
        (icons::network(), style::teal())
    } else if lower.contains("ruleset") {
        (icons::list_rect(), style::warning())
    } else {
        (icons::grid_fill(), style::indigo())
    }
}

pub fn rules_tab(m: Model) -> impl IntoElement {
    let search = signal(String::new());
    let type_filter = signal(RuleTypeFilter::All);
    let policy = signal(String::new());
    let grouped = signal(false);
    let collapsed = signal(std::collections::HashSet::<String>::new());
    let present = {
        let m = m.clone();
        move || m.rules.with(|items| data::present_rules(items, &search.get(), type_filter.get(), &policy.get(), grouped.get()))
    };
    let rows = {
        let present = present.clone();
        move || {
            let p = present();
            if grouped.get() {
                let collapsed = collapsed.get();
                let mut out = Vec::new();
                for (policy, rules) in p.groups {
                    let expanded = !collapsed.contains(&policy);
                    out.push(RuleRow::Header { policy: policy.clone(), count: rules.len(), expanded });
                    if expanded {
                        out.extend(rules.into_iter().map(RuleRow::Rule));
                    }
                }
                out
            } else {
                p.rules.into_iter().map(RuleRow::Rule).collect()
            }
        }
    };
    let rows = Rc::new(rows);
    let stat = |title: Rc<dyn Fn() -> String>, value: Rc<dyn Fn() -> String>| {
        view()
            .flex_row()
            .items_center()
            .gap(px(style::SPACE4))
            .child(
                text(move || title().to_uppercase())
                    .text_size(px(font::CAPTION))
                    .font_weight(FontWeight::BOLD)
                    .text_color(style::tertiary_label()),
            )
            .child(
                text(move || value())
                    .text_size(px(font::BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .tabular_nums()
                    .text_color(style::primary_label()),
            )
    };
    let (s1, s2, s3, s4) = (m.clone(), m.clone(), m.clone(), m.clone());
    let refresh_m = m.clone();
    let header = view()
        .flex_row()
        .items_center()
        .py(px(style::SPACE6))
        .gap(px(style::SPACE8))
        .child(stat(Rc::new(move || s1.tr("ui.rule.stats.rules")), Rc::new(move || s2.rules_count.get().to_string())))
        .child(stat(Rc::new(move || s3.tr("ui.rule.stats.sets")), Rc::new(move || s4.rule_providers.with(|p| p.len()).to_string())))
        .child(view().flex_1())
        .child(
            icon_button(icons::group_rects, move || if grouped.get() { style::info() } else { style::secondary_label() }, {
                let m = m.clone();
                move || m.tr(if grouped.get() { "ui.rules.group.on" } else { "ui.rules.group.off" })
            }, move || grouped.set(!grouped.get())),
        )
        .child(icon_button(
            {
                let m = m.clone();
                move || if m.rule_providers_refreshing.get() { icons::timer() } else { icons::arrows_circlepath() }
            },
            style::info,
            {
                let m = m.clone();
                move || m.tr("ui.action.refresh")
            },
            move || {
                let m = refresh_m.clone();
                crate::model::run_root(async move { m.refresh_rule_providers().await });
            },
        ));
    let type_chips: Vec<_> = RuleTypeFilter::ALL
        .iter()
        .map(|&f| {
            let m = m.clone();
            let present = present.clone();
            let on = move || type_filter.get() == f;
            let on2 = on.clone();
            text(move || {
                let count = present().type_counts.get(f.title_key()).copied().unwrap_or(0);
                format!("{} {count}", m.tr(f.title_key()))
            })
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || if on() { style::info() } else { style::secondary_label() })
            .px(px(style::SPACE6))
            .py(px(1.0))
            .rounded(px(8.0))
            .bg(move || if on2() { style::accent(0.16) } else { style::fill(0.05) })
            .on_press(move || type_filter.set(f))
        })
        .collect();
    let policy_m = m.clone();
    let policy_menu = icon_button(icons::filter_lines, move || if policy.with(|p| p.is_empty()) { style::secondary_label() } else { style::info() }, {
        let m = m.clone();
        move || m.tr("ui.rules.filter.policy")
    }, {
        let present = present.clone();
        move || {
            let current = policy.get_untracked();
            let mut items = vec![MenuItem::new(policy_m.tr("ui.rules.policy.all")).checked(current.is_empty()).on_select(move || policy.set(String::new()))];
            for name in present().policies {
                let n = name.clone();
                items.push(MenuItem::new(name.clone()).checked(current == name).on_select(move || policy.set(n.clone())));
            }
            let _ = popup_menu(items);
        }
    });
    let place_m = m.clone();
    let controls = control_card()
        .child(view().flex_row().items_center().gap(px(style::SPACE4)).children(type_chips).child(view().flex_1()).child(policy_menu))
        .child(filter_field(move || place_m.tr("ui.placeholder.filter_rule"), search));
    let column = |key: &'static str, w: Option<f32>, m: &Model| {
        let m = m.clone();
        let t = text(move || m.tr(key))
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(style::tertiary_label());
        match w {
            Some(w) => t.w(px(w)),
            None => t.flex_1(),
        }
    };
    let columns = view()
        .flex_row()
        .py(px(style::SPACE4))
        .child(view().w(px(24.0)))
        .child(column("ui.rules.column.target_type", Some(120.0 + style::SPACE6), &m))
        .child(column("ui.rules.column.policy", Some(90.0 + style::SPACE6), &m))
        .child(column("ui.rules.column.stats", None, &m));
    let empty_m = m.clone();
    let list_rows = rows.clone();
    let count_rows = rows.clone();
    let list_m = m.clone();
    view()
        .flex_1()
        .child(header)
        .child(separator())
        .child(view().py(px(style::SPACE6)).child(controls))
        .child(separator())
        .child(columns)
        .child(when(
            {
                let rows = rows.clone();
                move || rows().is_empty()
            },
            move || {
                let m = empty_m.clone();
                text(move || m.tr(if m.rules.with(|r| r.is_empty()) { "ui.empty.rules" } else { "ui.empty.rules.no_match" }))
                    .px(px(style::SPACE4))
                    .py(px(style::SPACE8))
                    .text_size(px(font::BODY))
                    .text_color(style::secondary_label())
            },
        ))
        .child(
            virtual_list(move || count_rows().len(), move |at: nsh_core::element::Position| {
                let row = {
                    let list_rows = list_rows.clone();
                    computed(move || list_rows().get(at.index()).cloned())
                };
                rule_row(list_m.clone(), row, collapsed)
            })
            .estimated_item_size(px(36.0))
            .flex_1(),
        )
}

fn rule_row(m: Model, row: Memo<Option<RuleRow>>, collapsed: Signal<std::collections::HashSet<String>>) -> impl IntoElement {
    let is_header = move || matches!(row.get(), Some(RuleRow::Header { .. }));
    let header_m = m.clone();
    view()
        .child(when(is_header.clone(), move || {
            let m = header_m.clone();
            let policy = move || match row.get() {
                Some(RuleRow::Header { policy, .. }) => policy,
                _ => String::new(),
            };
            let expanded = move || matches!(row.get(), Some(RuleRow::Header { expanded: true, .. }));
            let toggle_policy = policy.clone();
            view()
                .flex_row()
                .items_center()
                .h(px(26.0))
                .rounded(px(style::CORNER_RADIUS))
                .bg(style::fill(0.04))
                .on_press(move || {
                    let p = toggle_policy();
                    collapsed.update(|c| {
                        if !c.remove(&p) {
                            c.insert(p);
                        }
                    });
                })
                .child(view().w(px(24.0)).items_center().child(image(move || ImageSrc::from(if expanded() { icons::chevron_down() } else { icons::chevron_right() })).w(px(9.0)).h(px(9.0)).tint_color(style::secondary_label())))
                .child(
                    text({
                        let m = m.clone();
                        let policy = policy.clone();
                        move || {
                            let p = policy();
                            if p.is_empty() { m.tr("ui.common.na") } else { p }
                        }
                    })
                    .flex_1()
                    .truncate()
                    .text_size(px(font::BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(style::primary_label()),
                )
                .child(badge(move || match row.get() {
                    Some(RuleRow::Header { count, .. }) => count.to_string(),
                    _ => String::new(),
                }))
        }))
        .child(when(move || !is_header(), move || rule_item_row(m.clone(), row)))
}

fn rule_item_row(m: Model, row: Memo<Option<RuleRow>>) -> impl IntoElement {
    let rule = move || match row.get() {
        Some(RuleRow::Rule(r)) => r,
        _ => RuleItem { kind: None, payload: None, proxy: None },
    };
    let na = {
        let m = m.clone();
        move || m.tr("ui.common.na")
    };
    let stats = {
        let m = m.clone();
        let rule = rule.clone();
        move || {
            let payload = rule().payload.unwrap_or_default().trim().to_lowercase();
            if payload.is_empty() {
                return None;
            }
            m.rule_providers.with(|providers| {
                providers
                    .iter()
                    .find(|(k, d)| k.to_lowercase() == payload || d.name.as_deref().is_some_and(|n| n.to_lowercase() == payload))
                    .map(|(_, d)| (d.rule_count.unwrap_or(0), d.updated_at.clone()))
            })
        }
    };
    let (r1, r2, r3, r4, r5) = (rule.clone(), rule.clone(), rule.clone(), rule.clone(), rule.clone());
    let (n1, n2, n3) = (na.clone(), na.clone(), na.clone());
    let stats2 = stats.clone();
    view()
        .flex_row()
        .items_center()
        .py(px(style::SPACE4))
        .child(
            view().w(px(24.0)).child(
                image(move || ImageSrc::from(rule_type_icon(r1().kind.as_deref().unwrap_or("")).0))
                    .w(px(12.0))
                    .h(px(12.0))
                    .tint_color(move || rule_type_icon(r2().kind.as_deref().unwrap_or("")).1),
            ),
        )
        .child(
            view()
                .w(px(120.0 + style::SPACE6))
                .pr(px(style::SPACE6))
                .child(
                    text(move || r3().payload.filter(|p| !p.trim().is_empty()).unwrap_or_else(&n1))
                        .truncate()
                        .text_size(px(font::CALLOUT))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(style::primary_label()),
                )
                .child(
                    text(move || r4().kind.filter(|k| !k.trim().is_empty()).unwrap_or_else(&n2).to_uppercase())
                        .truncate()
                        .text_size(px(font::CAPTION))
                        .text_color(style::tertiary_label()),
                ),
        )
        .child(
            text(move || r5().proxy.filter(|p| !p.trim().is_empty()).unwrap_or_else(&n3))
                .w(px(90.0 + style::SPACE6))
                .pr(px(style::SPACE6))
                .truncate()
                .text_size(px(font::CAPTION))
                .font_weight(FontWeight::MEDIUM)
                .text_color(style::secondary_label()),
        )
        .child(
            view()
                .flex_1()
                .child(
                    text(move || stats().map(|(c, _)| c.to_string()).unwrap_or_default())
                        .text_size(px(font::CAPTION))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(style::secondary_label()),
                )
                .child(
                    text(move || match stats2() {
                        Some((_, updated)) => match format::relative_time(updated.as_deref(), format::now_secs()) {
                            Relative::Unknown => m.tr("fmt.common.unknown"),
                            Relative::Minutes(v) => m.tr_args("fmt.relative.minutes", &[&v]),
                            Relative::Hours(v) => m.tr_args("fmt.relative.hours", &[&v]),
                            Relative::Days(v) => m.tr_args("fmt.relative.days", &[&v]),
                        },
                        None => String::new(),
                    })
                    .text_size(px(font::CAPTION))
                    .text_color(style::tertiary_label()),
                ),
        )
}
