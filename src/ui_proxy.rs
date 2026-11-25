//! The proxy tab (ClashBar's `ProxyTabView` and
//! `ProxyProvidersAndGroupsView`): the traffic overview, the quick rows,
//! the proxy providers and the proxy groups; and the popovers attached
//! beside the panel (a group's nodes, the configs).

use std::rc::Rc;

use nsh_core::element::{ImageSrc, input};
use nsh_core::element::desktop::WindowRef;
use nsh_core::prelude::*;

use crate::format::{self, Expiry, Relative};
use crate::icons;
use crate::model::{HISTORY_MAX_POINTS, Model, SideKind, SidePopover, TrafficHistory};
use crate::proxy::{self, ProviderDetail, ProxyGroup, latency_tone};
use crate::style::{self, font};
use crate::widgets;

fn icon_view(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

fn icon_button(
    src: impl Fn() -> String + 'static,
    tone: impl Fn() -> AdaptiveColor + 'static,
    label: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    widgets::icon_button(src, tone, label, on_press)
}

thread_local! {
    /// The hover that may open a group's nodes (a newer one, or leaving
    /// the row, cancels it).
    static HOVER_OPEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Opens `kind` beside the panel (or keeps it open) without toggling: a
/// hover over a submenu's row.
fn show_side(m: &Model, win: &WindowRef, row: ElementRef, kind: SideKind) {
    if m.side_popover.with_untracked(|p| p.as_ref().is_some_and(|p| p.kind == kind)) {
        return;
    }
    let m = m.clone();
    let win = win.clone();
    crate::model::run_root(async move {
        let Some(measured) = row.measure().await else { return };
        let top = win.state().map_or(0.0, |s| s.content_bounds.origin.y.0);
        let mid = top + measured.page.y.0 + measured.frame.size.height.0 / 2.0;
        // Measured afresh as it shows.
        m.side_measured.set(0.0);
        m.side_popover.set(Some(SidePopover { kind, anchor_mid_y: mid }));
    });
}

/// Closes a group's nodes when the pointer moves to the panel's other
/// rows (a native menu closes a submenu when another item is hovered).
fn close_nodes_on_hover(m: &Model) -> impl Fn() + 'static {
    let m = m.clone();
    move || {
        HOVER_OPEN.with(|h| h.set(h.get() + 1));
        if m.side_popover.with_untracked(|p| matches!(p, Some(SidePopover { kind: SideKind::Nodes(_), .. }))) {
            m.side_popover.set(None);
        }
    }
}

/// Closes the config menu when the pointer rests on another row (its
/// import form stays: it may hold what was typed).
fn close_configs_on_hover(m: &Model) -> impl Fn(&MouseEvent) + 'static {
    let m = m.clone();
    move |_: &MouseEvent| {
        HOVER_OPEN.with(|h| h.set(h.get() + 1));
        if m.side_popover.with_untracked(|p| matches!(p, Some(SidePopover { kind: SideKind::Configs, .. }))) {
            m.side_popover.set(None);
        }
    }
}

/// A submenu's row: what it opens shows while the pointer rests on the
/// row (a native menu's short delay), at once when a submenu already shows
/// (it follows the pointer), and leaving the row cancels a pending one.
/// `row_height` places it level with the row.
fn hover_opens(m: &Model, win: &WindowRef, row: View, row_height: f32, kind: impl Fn() -> SideKind + 'static) -> View {
    let (m, win) = (m.clone(), win.clone());
    row.on_mouse_enter(move |e: &MouseEvent| {
        let token = HOVER_OPEN.with(|h| {
            h.set(h.get() + 1);
            h.get()
        });
        // The row's middle on the screen, from the event (no measuring
        // round trip).
        let row_top = e.page_position.y.0 - e.position.y.0;
        let top = win.state().map_or(0.0, |s| s.content_bounds.origin.y.0);
        let open = SidePopover { kind: kind(), anchor_mid_y: top + row_top + row_height / 2.0 };
        let showing = m.side_popover.with_untracked(|p| matches!(p, Some(SidePopover { kind: SideKind::Nodes(_) | SideKind::Configs, .. })));
        if m.side_popover.with_untracked(|p| p.as_ref().is_some_and(|p| p.kind == open.kind)) {
            return;
        }
        if showing {
            m.side_measured.set(0.0);
            m.side_popover.set(Some(open));
        } else {
            let m = m.clone();
            set_timeout(std::time::Duration::from_millis(90), move || {
                if HOVER_OPEN.with(|h| h.get()) == token {
                    m.side_measured.set(0.0);
                    m.side_popover.set(Some(open));
                }
            });
        }
    })
    .on_mouse_leave(move |_: &MouseEvent| HOVER_OPEN.with(|h| h.set(h.get() + 1)))
}

pub fn proxy_tab(m: Model, win: WindowRef) -> impl IntoElement {
    let providers_m = m.clone();
    view()
        .gap(px(10.0))
        .child(
            view()
                .gap(px(10.0))
                .on_mouse_enter({
                    let close = close_nodes_on_hover(&m);
                    move |_: &MouseEvent| close()
                })
                .child(view().on_mouse_enter(close_configs_on_hover(&m)).child(traffic_overview(m.clone())))
                .child(quick_rows(m.clone(), win.clone())),
        )
        .child(when(
            {
                let m = m.clone();
                move || m.providers.with(|p| !p.is_empty())
            },
            move || providers_section(providers_m.clone()),
        ))
        .child(groups_section(m, win))
}

// --- Traffic overview -------------------------------------------------------

/// The sparkline's paths (ClashBar's `TrafficSparklineView`): upload
/// above the axis, download below, the newest sample at the right. SVG
/// path data: the up area, the down area, the up line, the down line.
fn sparkline_paths(h: &TrafficHistory, width: f32, height: f32) -> [String; 4] {
    let max_y = h.up.iter().chain(h.down.iter()).copied().max().unwrap_or(0).max(1) as f32;
    let axis = (height * 0.5).floor();
    let upper = (axis - 2.0).max(1.0);
    let lower = (height - axis - 2.0).max(1.0);
    let step = width / (HISTORY_MAX_POINTS as f32 - 1.0);
    let points = |values: &std::collections::VecDeque<i64>, up: bool| -> Vec<(f32, f32)> {
        let n = values.len();
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let x = width - (n - 1 - i) as f32 * step;
                let ratio = (v.max(0) as f32).min(max_y) / max_y;
                let y = if up { axis - ratio * upper } else { axis + ratio * lower };
                (x, y)
            })
            .collect()
    };
    let line = |pts: &[(f32, f32)]| -> String {
        pts.iter().enumerate().map(|(i, (x, y))| format!("{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" })).collect()
    };
    let area = |pts: &[(f32, f32)]| -> String {
        match (pts.first(), pts.last()) {
            (Some(first), Some(last)) => format!("{}L{:.1} {axis}L{:.1} {axis}Z", line(pts), last.0, first.0),
            _ => String::new(),
        }
    };
    let (up, down) = (points(&h.up, true), points(&h.down, false));
    [area(&up), area(&down), line(&up), line(&down)]
}

fn hsla(hex: u32, alpha: f32) -> Hsla {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: alpha,
    }
    .into()
}

/// The sparkline drawn on a canvas: one drawing, redrawn as the history
/// changes (an image a second would fill the image caches).
fn sparkline(m: Model, width: f32, height: f32) -> impl IntoElement {
    canvas(move || {
        let [up_area, down_area, up_line, down_line] = m.history.with(|h| sparkline_paths(h, width, height));
        let axis = (height * 0.5).floor();
        let gradient = |hex: u32, top: f32, bottom: f32| {
            nsh_core::canvas::CanvasGradient::linear(point(px(0.0), px(0.0)), point(px(0.0), px(height)))
                .add_color_stop(percentage(0.0), hsla(hex, top))
                .add_color_stop(percentage(1.0), hsla(hex, bottom))
        };
        let mut painter = Painter::new()
            .begin_path()
            .move_to(0.0, axis)
            .line_to(width, axis)
            .stroke_color(hsla(0x8e8e93, 0.35))
            .line_width(0.65)
            .stroke();
        if !up_area.is_empty() {
            painter = painter.fill_style(gradient(0x0a84ff, 0.30, 0.02)).fill_svg(&up_area);
        }
        if !down_area.is_empty() {
            painter = painter.fill_style(gradient(0x30d158, 0.32, 0.0)).fill_svg(&down_area);
        }
        painter = painter.line_width(1.2);
        if !up_line.is_empty() {
            painter = painter.stroke_color(hsla(0x0a84ff, 0.9)).stroke_svg(&up_line);
        }
        if !down_line.is_empty() {
            painter = painter.stroke_color(hsla(0x30d158, 0.9)).stroke_svg(&down_line);
        }
        painter
    })
    .w(px(width))
    .h(px(height))
}

/// A rate with its arrow, and the total under it.
fn rate_stat(
    arrow: String,
    color: fn() -> AdaptiveColor,
    rate: impl Fn() -> String + 'static,
    total: impl Fn() -> String + 'static,
) -> impl IntoElement {
    view()
        .gap(px(1.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(widgets::icon(arrow, 12.0, color))
                .child(text(rate).text_size(px(font::TITLE3)).font_weight(FontWeight::SEMIBOLD).tabular_nums().text_color(style::primary_label())),
        )
        .child(text(total).pl(px(17.0)).text_size(px(font::CAPTION)).tabular_nums().text_color(style::secondary_label()))
}

/// A small metric chip (connections, memory).
fn metric_chip(icon: String, color: fn() -> AdaptiveColor, value: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(6.0))
        .h(px(18.0))
        .rounded(px(9.0))
        .bg(style::control_track())
        .child(widgets::icon(icon, 10.0, color))
        .child(text(value).text_size(px(font::CAPTION)).font_weight(FontWeight::MEDIUM).tabular_nums().text_color(style::secondary_label()))
}

fn traffic_overview(m: Model) -> impl IntoElement {
    const HEIGHT: f32 = 40.0;
    let width = style::PANEL_WIDTH - style::SPACE8 * 2.0 - 10.0 * 2.0 - 2.0;
    let (up_m, up_total_m, down_m, down_total_m, conn_m, mem_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    widgets::card()
        .p(px(10.0))
        .gap(px(style::SPACE6))
        .child(
            view()
                .flex_row()
                .items_start()
                .gap(px(14.0))
                .child(rate_stat(
                    icons::arrow_up_circle(),
                    style::info,
                    move || format::speed(up_m.traffic.get().up),
                    move || format::bytes_compact(up_total_m.history.with(|h| h.up_total)),
                ))
                .child(rate_stat(
                    icons::arrow_down_circle(),
                    style::positive,
                    move || format::speed(down_m.traffic.get().down),
                    move || format::bytes_compact(down_total_m.history.with(|h| h.down_total)),
                ))
                .child(view().flex_1())
                .child(
                    view()
                        .gap(px(4.0))
                        .items_end()
                        .child(metric_chip(icons::link(), style::indigo, move || conn_m.connections_count.get().to_string()))
                        .child(metric_chip(icons::memorychip(), style::teal, move || format::bytes_integer(mem_m.memory.get()))),
                ),
        )
        .child(sparkline(m, width, HEIGHT))
}

// --- Quick rows --------------------------------------------------------------

fn quick_row(
    icon: String,
    color: impl Fn() -> AdaptiveColor + 'static,
    title: impl Fn() -> String + 'static,
    trailing: impl IntoElement,
) -> nsh_core::element::View {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(style::SPACE6))
        .min_h(px(36.0))
        .rounded(px(style::ROW_RADIUS))
        .child(widgets::tile(icon, color))
        .child(
            text(title)
                .text_size(px(font::BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(style::primary_label()),
        )
        .child(view().flex_1())
        .child(trailing)
}

/// Where a card's dividers start: past the row's tile.
const ROW_INSET: f32 = style::SPACE6 + style::TILE + 10.0;

/// The outbound mode row: the current mode, and the modes in a native
/// submenu opened beside the row (rule / global / direct).
fn mode_row(m: Model, win: WindowRef) -> impl IntoElement {
    let row = element_ref();
    let (detail_m, press_m, dim_m) = (m.clone(), m.clone(), m.clone());
    let _ = win;
    quick_row(
        icons::shuffle(),
        style::indigo,
        {
            let m = m.clone();
            move || m.tr("ui.quick.outbound_mode")
        },
        view()
            .flex_row()
            .items_center()
            .gap(px(style::SPACE2))
            .child(
                text(move || match detail_m.mode.get() {
                    Some(mode) => detail_m.tr(mode.title_key()),
                    None => "--".into(),
                })
                .text_size(px(font::CAPTION))
                .text_color(style::secondary_label()),
            )
            .child(icon_view(icons::chevron_right(), 9.0, style::tertiary_label)),
    )
    .element_ref(&row)
    .hover(|s| s.bg(style::fill(0.05)))
    .opacity(move || if dim_m.is_mode_switch_enabled() { 1.0_f32 } else { 0.5_f32 })
    .on_press(move || {
        let m = press_m.clone();
        crate::model::run_root(async move {
            use nsh_core::element::desktop::{MenuItem, popup_menu, popup_menu_at};
            let current = m.mode.get_untracked();
            let enabled = m.is_mode_switch_enabled();
            let items = crate::model::CoreMode::ALL
                .iter()
                .map(|&mode| {
                    let m = m.clone();
                    MenuItem::new(m.tr(mode.title_key()))
                        .checked(current == Some(mode))
                        .disabled(!enabled)
                        .on_select(move || {
                            let m = m.clone();
                            crate::model::run_root(async move { m.switch_mode(mode).await });
                        })
                })
                .collect();
            // Beside the row, where a submenu opens.
            match row.measure().await {
                Some(r) => {
                    let at = point(px(r.page.x.0 + r.frame.size.width.0 - 6.0), px(r.page.y.0));
                    let _ = popup_menu_at(items, at);
                }
                None => {
                    let _ = popup_menu(items);
                }
            }
        });
    })
}

fn quick_rows(m: Model, win: WindowRef) -> impl IntoElement {
    let mode_win = win.clone();
    let config_ref = element_ref();
    let config_m = m.clone();
    let switch_config = quick_row(
        icons::doc_text(),
        style::purple,
        {
            let m = m.clone();
            move || m.tr("ui.quick.switch_config")
        },
        view()
            .flex_row()
            .items_center()
            .gap(px(style::SPACE2))
            .max_w(px(180.0))
            .child(
                text({
                    let m = m.clone();
                    move || m.selected_config_name().unwrap_or_else(|| m.tr("ui.common.unknown"))
                })
                .truncate()
                .text_size(px(font::CAPTION))
                .text_color(style::secondary_label()),
            )
            .child(icon_view(icons::chevron_right(), 9.0, style::tertiary_label)),
    )
    .element_ref(&config_ref)
    .hover(|s| s.bg(style::fill(0.05)))
    .bg({
        let m = m.clone();
        move || if m.side_popover.with(|p| matches!(p, Some(SidePopover { kind: SideKind::Configs | SideKind::ImportRemote, .. }))) { style::accent(0.13) } else { style::fill(0.0) }
    })
    .on_press({
        let win = win.clone();
        move || show_side(&config_m, &win, config_ref, SideKind::Configs)
    });
    // A submenu, as a group's row: it opens on hover.
    let switch_config = hover_opens(&m, &win, switch_config, 36.0, || SideKind::Configs);

    let copy_m = m.clone();
    let copied = signal(false);
    let copy_terminal = quick_row(
        icons::terminal(),
        style::warning,
        {
            let m = m.clone();
            move || m.tr("ui.quick.copy_terminal")
        },
        view()
            .flex_row()
            .items_center()
            .gap(px(style::SPACE4))
            .h(px(20.0))
            .px(px(7.0))
            .rounded(px(10.0))
            .bg(move || if copied.get() { style::positive().opacity(0.16) } else { style::control_track() })
            .tooltip(crate::widgets::tip({
                let m = m.clone();
                move || format!("{}\n{}", m.tr("ui.quick.copy_terminal"), m.local_proxy_target_display())
            }))
            .on_press(move || {
                copy_m.copy_proxy_command("127.0.0.1");
                copied.set(true);
                set_timeout(std::time::Duration::from_millis(1200), move || copied.set(false));
            })
            .child(
                text("127.0.0.1")
                    .text_size(px(font::CAPTION))
                    .font_weight(FontWeight::MEDIUM)
                    .tabular_nums()
                    .text_color(move || if copied.get() { style::positive() } else { style::secondary_label() }),
            )
            .child(
                image(move || ImageSrc::from(if copied.get() { icons::checkmark_circle_fill() } else { icons::doc_on_doc() }))
                    .w(px(10.0))
                    .h(px(10.0))
                    .tint_color(move || if copied.get() { style::positive() } else { style::tertiary_label() }),
            ),
    );
    // The other rows close the config menu as the pointer reaches them.
    let closes = |row: AnyElement| view().on_mouse_enter(close_configs_on_hover(&m)).child(row);
    widgets::card()
        .child(closes(mode_row(m.clone(), mode_win).into_element()))
        .child(widgets::divider(ROW_INSET))
        .child(switch_config)
        .child(widgets::divider(ROW_INSET))
        .child(closes(system_proxy_row(m.clone()).into_element()))
        .child(widgets::divider(ROW_INSET))
        .child(closes(tun_row(m.clone()).into_element()))
        .child(widgets::divider(ROW_INSET))
        .child(closes(copy_terminal.into_element()))
}

// --- Section headers ---------------------------------------------------------

fn section_header(
    _icon: String,
    title: impl Fn() -> String + 'static,
    count: impl Fn() -> String + 'static,
    trailing: impl IntoElement,
) -> impl IntoElement {
    widgets::section_title(title, Some(Rc::new(count)), trailing)
}

// --- Providers -----------------------------------------------------------------

fn providers_section(m: Model) -> impl IntoElement {
    let collapsed = {
        let m = m.clone();
        move || m.settings.with(|s| s.proxy_providers_collapsed)
    };
    let toggle_m = m.clone();
    let rows_m = m.clone();
    view()
        .gap(px(style::SPACE6))
        .child(section_header(
            icons::externaldrive(),
            {
                let m = m.clone();
                move || m.tr("ui.section.proxy_providers")
            },
            {
                let m = m.clone();
                move || m.providers.with(|p| p.len()).to_string()
            },
            icon_button(
                {
                    let collapsed = collapsed.clone();
                    move || if collapsed() { icons::chevron_right() } else { icons::chevron_down() }
                },
                style::secondary_label,
                {
                    let m = m.clone();
                    let collapsed = collapsed.clone();
                    move || m.tr(if collapsed() { "ui.action.expand" } else { "ui.action.collapse" })
                },
                move || toggle_m.settings.update(|s| s.proxy_providers_collapsed = !s.proxy_providers_collapsed),
            ),
        ))
        .child(when(move || !collapsed(), move || {
            let m = rows_m.clone();
            widgets::card().child(each(
                {
                    let m = m.clone();
                    move || m.providers.with(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>())
                },
                |(name, _): &(String, ProviderDetail)| name.clone(),
                move |entry, _| provider_row(m.clone(), entry),
            ))
        }))
}

fn provider_row(m: Model, entry: ReadSignal<(String, ProviderDetail)>) -> impl IntoElement {
    let name = move || entry.with(|(n, _)| n.clone());
    let detail = move || entry.with(|(_, d)| d.clone());
    let updating = {
        let m = m.clone();
        move || m.provider_updating.with(|s| s.contains(&name()))
    };
    let press_m = m.clone();
    let has_subscription = move || detail().subscription_info.is_some();
    let sub_m = m.clone();
    view()
        .gap(px(style::SPACE6))
        .px(px(style::SPACE6))
        .py(px(style::SPACE6))
        .rounded(px(style::ROW_RADIUS))
        .hover(|s| s.bg(style::fill(0.04)))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(widgets::tile(icons::externaldrive(), style::teal))
                .child(
                    view()
                        .flex_1()
                        .flex_row()
                        .items_center()
                        .gap(px(style::SPACE4))
                        .child(
                            text(name)
                                .truncate()
                                .text_size(px(font::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(style::primary_label()),
                        )
                        .child(
                            text(move || detail().proxies.map_or(0, |p| p.len()).to_string())
                                .text_size(px(font::CAPTION))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(style::secondary_label()),
                        ),
                )
                .child(
                    text({
                        let m = m.clone();
                        move || match format::relative_time(detail().updated_at.as_deref(), format::now_secs()) {
                            Relative::Unknown => m.tr("fmt.common.unknown"),
                            Relative::Minutes(v) => m.tr_args("fmt.relative.minutes", &[&v]),
                            Relative::Hours(v) => m.tr_args("fmt.relative.hours", &[&v]),
                            Relative::Days(v) => m.tr_args("fmt.relative.days", &[&v]),
                        }
                    })
                    .min_w(px(44.0))
                    .text_right()
                    .text_size(px(font::CAPTION))
                    .text_color(style::tertiary_label()),
                )
                .child(icon_button(
                    {
                        let updating = updating.clone();
                        move || if updating() { icons::timer() } else { icons::arrows_circlepath() }
                    },
                    style::secondary_label,
                    {
                        let m = m.clone();
                        move || m.tr("ui.action.refresh")
                    },
                    move || {
                        let m = press_m.clone();
                        let name = name();
                        crate::model::run_root(async move { m.update_proxy_provider(name).await });
                    },
                )),
        )
        .child(when(has_subscription, move || {
            let m = sub_m.clone();
            let info = move || detail().subscription_info.unwrap_or_default();
            let ratio = move || {
                let i = info();
                match (i.total, i.upload, i.download) {
                    (Some(t), Some(u), Some(d)) if t > 0 => Some(((u + d) as f32 / t as f32).clamp(0.0, 1.0)),
                    _ => None,
                }
            };
            let bar_width = style::PANEL_WIDTH - 2.0 * style::SPACE8 - 2.0 * style::SPACE4 - 2.0 * style::SPACE6 - (style::TILE + 10.0) - (24.0 + style::SPACE6);
            view()
                .gap(px(style::SPACE2))
                .pl(px(style::TILE + 10.0))
                .pr(px(24.0 + style::SPACE6))
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .child(
                            text({
                                let m = m.clone();
                                move || match format::expiry(info().expire, format::now_secs()) {
                                    Expiry::Unknown => m.tr("fmt.common.unknown"),
                                    Expiry::LongTerm => m.tr("fmt.expiry_short.long_term"),
                                    Expiry::Expired => m.tr("fmt.expiry_short.expired"),
                                    Expiry::Today => m.tr("fmt.expiry_short.today"),
                                    Expiry::Days(d) => m.tr_args("fmt.expiry_short.days", &[&d]),
                                }
                            })
                            .text_size(px(font::CAPTION))
                            .text_color(move || if info().expire == Some(0) { style::secondary_label() } else { style::warning() }),
                        )
                        .child(
                            text(move || {
                                let i = info();
                                match (i.upload, i.download, i.total) {
                                    (Some(u), Some(d), Some(t)) => format!(
                                        "{} / {}",
                                        format::bytes_compact_no_space(u + d),
                                        format::bytes_compact_no_space(t)
                                    ),
                                    _ => String::new(),
                                }
                            })
                            .text_size(px(font::CAPTION))
                            .text_color(style::secondary_label()),
                        ),
                )
                .child(when(move || ratio().is_some(), move || {
                    view().h(px(style::SPACE6)).rounded(px(3.0)).bg(style::fill(0.10)).child(
                        view()
                            .h(px(style::SPACE6))
                            .rounded(px(3.0))
                            .w(move || px(bar_width * ratio().unwrap_or(0.0)))
                            .bg(move || {
                                let r = ratio().unwrap_or(0.0);
                                if r >= 0.9 {
                                    style::negative()
                                } else if r >= 0.75 {
                                    style::warning()
                                } else {
                                    style::info()
                                }
                            }),
                    )
                }))
        }))
}

// --- Groups --------------------------------------------------------------------

fn groups_section(m: Model, win: WindowRef) -> impl IntoElement {
    let setting = |m: &Model, f: fn(&crate::settings::Settings) -> bool| {
        let m = m.clone();
        move || m.settings.with(f)
    };
    let sort = setting(&m, |s| s.sort_nodes_by_latency);
    let history = setting(&m, |s| s.show_latency_history);
    let hide = setting(&m, |s| s.hide_hidden_proxy_groups);
    let tr = |m: &Model, f: Rc<dyn Fn() -> bool>, on: &'static str, off: &'static str| {
        let m = m.clone();
        move || m.tr(if f() { on } else { off })
    };
    let sort_rc: Rc<dyn Fn() -> bool> = Rc::new(sort.clone());
    let history_rc: Rc<dyn Fn() -> bool> = Rc::new(history.clone());
    let hide_rc: Rc<dyn Fn() -> bool> = Rc::new(hide.clone());
    let (sm, hm, xm, tm) = (m.clone(), m.clone(), m.clone(), m.clone());
    let actions = view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .child(icon_button(
            move || if sort() { icons::timer() } else { icons::list_number() },
            style::teal,
            tr(&m, sort_rc, "ui.action.sort_nodes_default", "ui.action.sort_nodes_by_latency"),
            move || sm.settings.update(|s| s.sort_nodes_by_latency = !s.sort_nodes_by_latency),
        ))
        .child(icon_button(
            move || if history() { icons::chart_bar_xaxis() } else { icons::chart_bar() },
            style::teal,
            tr(&m, history_rc, "ui.action.hide_latency_history", "ui.action.show_latency_history"),
            move || hm.settings.update(|s| s.show_latency_history = !s.show_latency_history),
        ))
        .child(icon_button(
            move || if hide() { icons::eye_slash() } else { icons::eye() },
            style::indigo,
            tr(&m, hide_rc, "ui.action.show_hidden_proxy_groups", "ui.action.hide_hidden_proxy_groups"),
            move || xm.settings.update(|s| s.hide_hidden_proxy_groups = !s.hide_hidden_proxy_groups),
        ))
        .child(icon_button(
            icons::gauge,
            style::teal,
            {
                let m = m.clone();
                move || m.tr("ui.action.test_latency")
            },
            move || {
                let m = tm.clone();
                let include_hidden = !m.settings.with_untracked(|s| s.hide_hidden_proxy_groups);
                crate::model::run_root(async move { m.refresh_all_group_latencies(include_hidden).await });
            },
        ));
    let empty_m = m.clone();
    let rows_m = m.clone();
    let list_m = m.clone();
    view()
        .gap(px(style::SPACE6))
        .child(section_header(
            icons::nodes_graph(),
            {
                let m = m.clone();
                move || m.tr("ui.section.proxy_groups")
            },
            {
                let m = m.clone();
                move || m.visible_groups().len().to_string()
            },
            actions,
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.visible_groups().is_empty()
            },
            move || empty_card(empty_m.tr("ui.empty.proxy_groups")),
        ))
        .child(when(
            {
                let m = list_m.clone();
                move || !m.visible_groups().is_empty()
            },
            move || {
                let (rows_m, m, win) = (rows_m.clone(), m.clone(), win.clone());
                widgets::card().child(each(
            move || rows_m.visible_groups(),
            |g: &ProxyGroup| g.name.clone(),
            move |group, _| group_row(m.clone(), win.clone(), group),
                ))
            },
        ))
}

pub fn empty_card(message: String) -> impl IntoElement {
    widgets::card()
        .items_center()
        .py(px(18.0))
        .child(text(message).text_size(px(font::CALLOUT)).text_color(style::secondary_label()))
}

/// The delay badge: signal bars (the last delay, or the last 4) and the
/// value (ClashBar's `delayBadge` and `LatencySignalBars`).
fn delay_badge(m: Model, samples: impl Fn() -> Vec<i64> + Clone + 'static) -> impl IntoElement {
    const HEIGHTS: [f32; 4] = [4.0, 7.0, 10.0, 13.0];
    let bars = (0..proxy::DELAY_HISTORY_LIMIT).map(|index| {
        let samples = samples.clone();
        let m = m.clone();
        view()
            .w(px(2.0))
            .h(px(HEIGHTS[index]))
            .rounded(px(0.75))
            .bg({
                let samples = samples.clone();
                move || {
                    let s = samples();
                    let offset = proxy::DELAY_HISTORY_LIMIT as isize - s.len() as isize;
                    let i = index as isize - offset;
                    if i >= 0 && (i as usize) < s.len() {
                        style::latency(latency_tone(Some(s[i as usize])))
                    } else {
                        style::fill(0.12)
                    }
                }
            })
            .opacity(move || {
                let shown = m.settings.with(|s| s.show_latency_history) || index == proxy::DELAY_HISTORY_LIMIT - 1;
                if shown { 1.0_f32 } else { 0.0_f32 }
            })
    });
    let value_m = m.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .child(view().flex_row().items_end().gap(px(1.5)).h(px(13.0)).children(bars.collect::<Vec<_>>()))
        .child(
            text(move || {
                let v = samples().last().copied();
                match v {
                    Some(v) if v > 0 => v.to_string(),
                    other => value_m.delay_text(other),
                }
            })
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::MEDIUM)
            .font_family("Menlo")
            .text_color(style::primary_label()),
        )
}

const GROUP_ROW_HEIGHT: f32 = 30.0;

fn group_row(m: Model, win: WindowRef, group: ReadSignal<ProxyGroup>) -> impl IntoElement {
    let row = element_ref();
    let name = move || group.with(|g| g.name.clone());
    let current = {
        let m = m.clone();
        move || group.with(|g| g.now.clone()).unwrap_or_else(|| m.tr("ui.common.na"))
    };
    let samples = {
        let m = m.clone();
        let current = current.clone();
        move || m.delay_samples_for(&name(), &current(), true)
    };
    let loading = {
        let m = m.clone();
        move || m.group_latency_loading.with(|s| s.contains(&name()))
    };
    let press_m = m.clone();
    let test_m = m.clone();
    let icon_url = move || group.with(|g| g.icon.clone());
    view()
        .element_ref(&row)
        .flex_row()
        .items_center()
        .gap(px(style::SPACE4))
        .h(px(GROUP_ROW_HEIGHT))
        .px(px(style::SPACE6))
        .rounded(px(style::ROW_RADIUS))
        .hover(|s| s.bg(style::fill(0.05)))
        .bg({
            let m = m.clone();
            move || {
                let open = m.side_popover.with(|p| {
                    p.as_ref().is_some_and(|p| p.kind == SideKind::Nodes(name()))
                });
                if open { style::accent(0.13) } else { style::fill(0.0) }
            }
        })
        // A submenu: the nodes open while the pointer rests on the row
        // (a native menu's delay), or on a press.
        .on_mouse_enter({
            let (m, win) = (press_m.clone(), win.clone());
            move |e: &MouseEvent| {
                let token = HOVER_OPEN.with(|h| {
                    h.set(h.get() + 1);
                    h.get()
                });
                // The row's middle on the screen, from the event itself
                // (no measuring round trip): its top is the pointer's page
                // position less its position in the row.
                let row_top = e.page_position.y.0 - e.position.y.0;
                let top = win.state().map_or(0.0, |s| s.content_bounds.origin.y.0);
                let anchor_mid_y = top + row_top + GROUP_ROW_HEIGHT / 2.0;
                let kind = SideKind::Nodes(name());
                let open = SidePopover { kind, anchor_mid_y };
                // A submenu already open follows the pointer at once; the
                // first opens after a native menu's short rest.
                let showing = m.side_popover.with_untracked(|p| matches!(p, Some(SidePopover { kind: SideKind::Nodes(_) | SideKind::Configs, .. })));
                if showing {
                    if !m.side_popover.with_untracked(|p| p.as_ref().is_some_and(|p| p.kind == open.kind)) {
                        m.side_popover.set(Some(open));
                    }
                } else {
                    let m = m.clone();
                    set_timeout(std::time::Duration::from_millis(90), move || {
                        if HOVER_OPEN.with(|h| h.get()) == token {
                            m.side_popover.set(Some(open));
                        }
                    });
                }
            }
        })
        .on_mouse_leave(move |_: &MouseEvent| HOVER_OPEN.with(|h| h.set(h.get() + 1)))
        .on_press(move || show_side(&press_m, &win, row, SideKind::Nodes(name())))
        .child(when(move || icon_url().is_some(), move || {
            image(move || ImageSrc::from(icon_url().unwrap_or_default())).w(px(16.0)).h(px(16.0))
        }))
        .child(
            text(name)
                .w(px(108.0))
                .truncate()
                .text_size(px(font::BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(style::primary_label()),
        )
        .child(
            view().flex_1().flex_row().child(
                text(current)
                    .truncate()
                    .text_size(px(font::SUBHEADLINE))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(style::secondary_label())
                    .px(px(7.0))
                    .py(px(2.0))
                    .rounded(px(9.0))
                    .bg(style::control_track()),
            ),
        )
        .child(view().w(px(64.0)).flex_row().justify_end().child(delay_badge(m.clone(), samples)))
        .child(icon_button(
            move || if loading() { icons::timer() } else { icons::gauge() },
            style::teal,
            {
                let m = m.clone();
                move || m.tr("ui.action.test_latency")
            },
            move || {
                let m = test_m.clone();
                let name = name();
                crate::model::run_root(async move { m.refresh_group_latency(name).await });
            },
        ))
}

// --- Attached popovers ---------------------------------------------------------

pub const SIDE_WIDTH: f32 = 280.0;

/// The height a side popover's content wants.
pub fn side_height(m: &Model, kind: &SideKind) -> f32 {
    // The config menu and the import form are as tall as they measure
    // (they change as subscriptions update or Wi-Fi switching shows its
    // state); a node list is sized by its rows and scrolls.
    if !matches!(kind, SideKind::Nodes(_)) {
        let measured = m.side_measured.get();
        if measured > 0.0 {
            // Its padding and hairline border around it.
            return measured + style::SPACE8 * 2.0 + 2.0;
        }
    }
    let rows = match kind {
        SideKind::Nodes(group) => m.groups.with_untracked(|gs| {
            gs.iter().find(|g| &g.name == group).map_or(0, |g| proxy::ordered_unique(&g.all).len())
        }),
        SideKind::Configs => m.configs.with_untracked(|c| c.len()) * 2 + 11,
        SideKind::ImportRemote => 12,
    };
    // Node rows are 26 points; the header and padding about 50.
    (50.0 + rows as f32 * 26.0 + 8.0).min(560.0)
}

/// `content` at its own height, reported for the popover's height, and
/// scrolling if the screen is shorter.
fn measured(m: Model, content: impl IntoElement) -> impl IntoElement {
    scroll().flex_1().scrollbar(false).child(
        view()
            .on_layout(move |b: &Bounds<Points>| {
                let h = b.size.height.0;
                if (m.side_measured.get_untracked() - h).abs() > 0.5 {
                    m.side_measured.set(h);
                }
            })
            .child(content),
    )
}

pub fn side_content(m: Model) -> impl IntoElement {
    let nodes_m = m.clone();
    let configs_m = m.clone();
    // 8 in from the 18pt corner: the 10pt cards sit concentric in it.
    style::window_surface(view())
        .flex_1()
        .p(px(style::SPACE8))
        .bg(style::menu_background())
        .child(when(
            {
                let m = m.clone();
                move || m.side_popover.with(|p| matches!(p, Some(SidePopover { kind: SideKind::Nodes(_), .. })))
            },
            move || nodes_list(nodes_m.clone()),
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.side_popover.with(|p| matches!(p, Some(SidePopover { kind: SideKind::Configs, .. })))
            },
            move || measured(configs_m.clone(), configs_menu(configs_m.clone())),
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.side_popover.with(|p| matches!(p, Some(SidePopover { kind: SideKind::ImportRemote, .. })))
            },
            {
                let m = m.clone();
                move || measured(m.clone(), import_remote_form(m.clone()))
            },
        ))

}

fn popover_header(title: impl Fn() -> String + 'static, count: impl Fn() -> String + 'static, trailing: impl IntoElement) -> impl IntoElement {
    view()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE4))
                .px(px(style::SPACE4))
                .pb(px(style::SPACE2))
                .child(
                    text(title)
                        .truncate()
                        .text_size(px(font::BODY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(style::primary_label()),
                )
                .child(
                    text(count)
                        .text_size(px(font::CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(style::secondary_label())
                        .px(px(style::SPACE4))
                        .py(px(1.0))
                        .rounded(px(style::ROW_RADIUS))
                        .bg(style::badge_fill()),
                )
                .child(view().flex_1())
                .child(trailing),
        )
        .child(view().h(px(0.65)).bg(style::separator()).mb(px(1.0)))
}

fn nodes_list(m: Model) -> impl IntoElement {
    let group_name = {
        let m = m.clone();
        move || match m.side_popover.with(|p| p.as_ref().map(|p| p.kind.clone())) {
            Some(SideKind::Nodes(g)) => g,
            _ => String::new(),
        }
    };
    let group = {
        let m = m.clone();
        let group_name = group_name.clone();
        move || m.groups.with(|gs| gs.iter().find(|g| g.name == group_name()).cloned())
    };
    let nodes = {
        let m = m.clone();
        let group = group.clone();
        move || {
            let Some(g) = group() else { return Vec::new() };
            let (sort, hide_unavailable) =
                m.settings.with(|s| (s.sort_nodes_by_latency, s.hide_unavailable_proxy_nodes));
            let latency = |n: &str| m.delay_value(&g.name, n);
            let list = if sort { proxy::sort_by_latency(&g.all, latency) } else { proxy::ordered_unique(&g.all) };
            if hide_unavailable { list.into_iter().filter(|n| latency(n).is_some_and(|v| v > 0)).collect() } else { list }
        }
    };
    let test_m = m.clone();
    let loading = {
        let m = m.clone();
        let group_name = group_name.clone();
        move || m.group_latency_loading.with(|s| s.contains(&group_name()))
    };
    let count = {
        let group = group.clone();
        move || group().map_or(0, |g| g.all.len()).to_string()
    };
    let header_name = group_name.clone();
    let list_m = m.clone();
    view()
        .flex_1()
        .child(popover_header(
            header_name,
            count,
            icon_button(
                move || if loading() { icons::timer() } else { icons::gauge() },
                style::teal,
                {
                    let m = m.clone();
                    move || m.tr("ui.action.test_latency")
                },
                {
                    let group_name = group_name.clone();
                    move || {
                        let m = test_m.clone();
                        let name = group_name();
                        crate::model::run_root(async move { m.refresh_group_latency(name).await });
                    }
                },
            ),
        ))
        .child(scroll().flex_1().scrollbar(false).child(view().gap(px(2.0)).child(each(
            nodes,
            |n: &String| n.clone(),
            move |node, _| node_row(list_m.clone(), group.clone(), node),
        ))))
}

fn node_row(
    m: Model,
    group: impl Fn() -> Option<ProxyGroup> + Clone + 'static,
    node: ReadSignal<String>,
) -> impl IntoElement {
    let group_name = {
        let group = group.clone();
        move || group().map(|g| g.name).unwrap_or_default()
    };
    let selected = {
        let group = group.clone();
        move || group().and_then(|g| g.now) == Some(node.get())
    };
    let samples = {
        let m = m.clone();
        let group_name = group_name.clone();
        move || m.delay_samples_for(&group_name(), &node.get(), false)
    };
    let testing = {
        let m = m.clone();
        let group_name = group_name.clone();
        move || m.proxy_latency_testing.with(|s| s.contains(&(group_name(), node.get())))
    };
    let kind = {
        let m = m.clone();
        move || m.node_types.with(|t| t.get(&node.get()).cloned())
    };
    let select_m = m.clone();
    let select_group = group_name.clone();
    let test_m = m.clone();
    let test_group = group_name.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .h(px(26.0))
        .px(px(style::SPACE4))
        .rounded(px(style::ROW_RADIUS))
        .bg({
            let selected = selected.clone();
            move || if selected() { style::accent(0.14) } else { style::fill(0.0) }
        })
        .hover(|s| s.bg(style::fill(0.05)))
        .child(
            view()
                .flex_1()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE4))
                .on_press(move || {
                    let m = select_m.clone();
                    let group = select_group();
                    let target = node.get_untracked();
                    m.side_popover.set(None);
                    crate::model::run_root(async move { m.switch_proxy(group, target).await });
                })
                .child(
                    image({
                        let selected = selected.clone();
                        move || ImageSrc::from(if selected() { icons::checkmark_circle_fill() } else { icons::circle() })
                    })
                    .w(px(11.0))
                    .h(px(11.0))
                    .tint_color({
                        let selected = selected.clone();
                        move || if selected() { style::info() } else { style::tertiary_label() }
                    }),
                )
                .child(
                    text(move || node.get())
                        .flex_1()
                        .truncate()
                        .text_size(px(font::BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color({
                            let selected = selected.clone();
                            move || if selected() { style::primary_label() } else { style::secondary_label() }
                        }),
                )
                .child(when(
                    {
                        let kind = kind.clone();
                        move || kind().is_some()
                    },
                    move || {
                        let kind = kind.clone();
                        text(move || kind().unwrap_or_default())
                            .text_size(px(font::CAPTION))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(style::secondary_label())
                            .px(px(style::SPACE4))
                            .py(px(1.0))
                            .rounded(px(style::CORNER_RADIUS))
                            .bg(style::fill(0.06))
                    },
                )),
        )
        .child(
            view()
                .w(px(56.0))
                .flex_row()
                .justify_end()
                .tooltip(crate::widgets::tip({
                    let m = m.clone();
                    move || m.tr("ui.action.test_latency")
                }))
                .on_press(move || {
                    let m = test_m.clone();
                    let group = test_group();
                    let node = node.get_untracked();
                    crate::model::run_root(async move { m.refresh_proxy_latency(group, node).await });
                })
                .child(when(
                    {
                        let testing = testing.clone();
                        move || testing()
                    },
                    || text("···").text_size(px(font::CAPTION)).text_color(style::tertiary_label()),
                ))
                .child(when(move || !testing(), move || delay_badge(m.clone(), samples.clone()))),
        )
}

/// The "switch config" menu (ClashBar's `configMenuContent`).
/// A row of the config menu's cards (the panel's quick rows): a tile, a
/// title, and what follows it.
fn menu_row(icon: String, color: fn() -> AdaptiveColor, title: impl Fn() -> String + 'static, on_press: impl Fn() + 'static) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(style::SPACE6))
        .min_h(px(34.0))
        .rounded(px(style::ROW_RADIUS))
        .hover(|s| s.bg(style::fill(0.05)))
        .on_press(on_press)
        .child(widgets::tile(icon, color))
        .child(text(title).flex_1().truncate().text_size(px(font::BODY)).font_weight(FontWeight::MEDIUM).text_color(style::primary_label()))
}

/// What the config menu just did: a line in green (done) or red (failed).
fn feedback_line(m: Model) -> impl IntoElement {
    let failed = {
        let m = m.clone();
        move || m.config_feedback.with(|f| f.as_ref().is_some_and(|(_, failed)| *failed))
    };
    let (f1, f2) = (failed.clone(), failed.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE4))
        .px(px(style::SPACE6))
        .py(px(style::SPACE4))
        .rounded(px(style::CORNER_RADIUS))
        .bg(move || if f1() { style::negative().opacity(0.12) } else { style::positive().opacity(0.12) })
        .child(image(move || ImageSrc::from(if f2() { icons::warning_triangle() } else { icons::checkmark_circle_fill() })).w(px(12.0)).h(px(12.0)).tint_color(move || if failed() { style::negative() } else { style::positive() }))
        .child(text(move || m.config_feedback.with(|f| f.as_ref().map(|(t, _)| t.clone()).unwrap_or_default())).flex_1().text_size(px(font::CAPTION)).text_color(style::primary_label()))
}

/// Rows in a card with inset dividers between them.
fn card_of(rows: Vec<AnyElement>) -> impl IntoElement {
    let n = rows.len();
    let mut card = widgets::card();
    for (i, row) in rows.into_iter().enumerate() {
        card = card.child(row);
        if i + 1 < n {
            card = card.child(widgets::divider(ROW_INSET));
        }
    }
    card
}

/// The config menu (ClashBar's): the local and remote configs to choose
/// from, what can be done with them, and Wi-Fi switching, as the panel's
/// cards.
fn configs_menu(m: Model) -> impl IntoElement {
    let local = {
        let m = m.clone();
        move || m.configs.get().into_iter().filter(|f| m.subscription(f).is_none()).collect::<Vec<_>>()
    };
    let remote = {
        let m = m.clone();
        move || m.configs.get().into_iter().filter(|f| m.subscription(f).is_some()).collect::<Vec<_>>()
    };
    let has_remote = {
        let remote = remote.clone();
        move || !remote().is_empty()
    };
    let list = |m: &Model, files: Rc<dyn Fn() -> Vec<String>>| {
        let m = m.clone();
        widgets::card().child(each(move || files(), |n: &String| n.clone(), move |name, index| {
            view()
                .child(when(move || index.get() > 0, || widgets::divider(ROW_INSET)))
                .child(config_menu_row(m.clone(), name))
        }))
    };
    let title = |m: &Model, key: &'static str, count: Rc<dyn Fn() -> String>| {
        let m = m.clone();
        widgets::section_title(move || m.tr(key), Some(count), view())
    };
    let (local_count, remote_count) = (local.clone(), remote.clone());
    let (reload_m, finder_m, import_m, remote_m, update_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    let tr = |m: &Model, key: &'static str| {
        let m = m.clone();
        move || m.tr(key)
    };
    // A row's mark while its action runs.
    let busy = |m: &Model, key: &'static str| {
        let m = m.clone();
        let shown = m.clone();
        when(move || shown.config_busy.get() == Some(key), move || icon_view(icons::timer(), 12.0, style::secondary_label))
    };
    let actions = card_of(vec![
        menu_row(icons::arrows_circlepath(), style::gray, tr(&m, "ui.quick.reload_config_list"), move || {
            if reload_m.config_busy.get_untracked().is_some() {
                return;
            }
            let m = reload_m.clone();
            crate::model::run_root(async move { m.reload_configs_and_core().await });
        })
        .child(busy(&m, "ui.quick.reload_config_list"))
        .into_element(),
        menu_row(icons::import_file(), style::info, tr(&m, "ui.quick.import_local_config"), move || {
            let m = import_m.clone();
            crate::model::run_root(async move { m.import_local_config().await });
        })
        .into_element(),
        menu_row(icons::link(), style::purple, tr(&m, "ui.quick.import_remote_config"), move || {
            let anchor = remote_m.side_popover.with_untracked(|p| p.as_ref().map_or(0.0, |p| p.anchor_mid_y));
            remote_m.side_measured.set(0.0);
            remote_m.side_popover.set(Some(SidePopover { kind: SideKind::ImportRemote, anchor_mid_y: anchor }));
        })
        .into_element(),
        menu_row(icons::arrow_down_circle_line(), style::positive, tr(&m, "ui.quick.update_remote_configs"), move || {
            if update_m.config_busy.get_untracked().is_some() {
                return;
            }
            let m = update_m.clone();
            crate::model::run_root(async move { m.update_all_remote_configs().await });
        })
        .child(busy(&m, "ui.quick.update_remote_configs"))
        .into_element(),
        menu_row(icons::folder(), style::warning, tr(&m, "ui.quick.show_in_finder"), move || finder_m.show_config_dir()).into_element(),
    ]);
    let remote_title_m = m.clone();
    let remote_list_m = m.clone();
    let feedback_m = m.clone();
    view()
        .gap(px(style::SPACE4))
        .child(when(move || feedback_m.config_feedback.with(Option::is_some), {
            let m = m.clone();
            move || feedback_line(m.clone())
        }))
        .child(title(&m, "ui.quick.configs.local_section", Rc::new(move || local_count().len().to_string())))
        .child(list(&m, Rc::new(local)))
        .child(when(has_remote, move || {
            let remote = remote.clone();
            let remote_count = remote_count.clone();
            view()
                .gap(px(style::SPACE4))
                .child(title(&remote_title_m, "ui.quick.configs.remote_section", Rc::new(move || remote_count().len().to_string())))
                .child(list(&remote_list_m, Rc::new(remote)))
        }))
        .child(view().h(px(style::SPACE2)))
        .child(actions)
        .child(ssid_strategy_controls(m.clone()))
}

/// Wi-Fi auto-switch in the config menu (ClashBar's SSID strategy items):
/// a switch, and while it is on, the permission and the current network.
fn ssid_strategy_controls(m: Model) -> impl IntoElement {
    let enabled = {
        let m = m.clone();
        move || m.settings.with(|s| s.ssid_strategy_enabled)
    };
    let (toggle_m, status_m, perm_m, perm2_m, title_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    let e1 = enabled.clone();
    let e2 = enabled.clone();
    widgets::card()
        .child(
            menu_row(icons::radiowaves(), style::teal, move || title_m.tr("ui.quick.ssid.auto_switch"), move || {
                let m = toggle_m.clone();
                let next = !m.settings.with_untracked(|s| s.ssid_strategy_enabled);
                crate::model::run_root(async move { m.set_ssid_strategy_enabled(next).await });
            })
            .child(widgets::switch(move || e1(), || false)),
        )
        .child(when(e2, move || {
            let m = status_m.clone();
            let (perm_m, perm2_m) = (perm_m.clone(), perm2_m.clone());
            view()
                .gap(px(2.0))
                .pl(px(ROW_INSET))
                .pr(px(style::SPACE6))
                .pb(px(style::SPACE4))
                .child(
                    text({
                        let m = m.clone();
                        move || {
                            let auth = m.tr(m.ssid_authorization.get().label_key());
                            let ssid = m.current_ssid.get().unwrap_or_else(|| m.tr("ui.settings.ssid_strategy.current_ssid.none"));
                            format!("{auth} · {ssid}")
                        }
                    })
                    .text_size(px(font::CAPTION))
                    .text_color(style::secondary_label()),
                )
                .child(text({
                    let m = m.clone();
                    move || m.tr("ui.quick.ssid.usage_hint")
                }).text_size(px(font::CAPTION)).text_color(style::tertiary_label()))
                .child(when(
                    move || perm_m.ssid_authorization.get() != crate::wifi_types::Authorization::Authorized,
                    move || {
                        let m = perm2_m.clone();
                        let label_m = m.clone();
                        text(move || label_m.tr("ui.quick.ssid.request_permission"))
                            .text_size(px(font::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(style::info())
                            .on_press(move || {
                                let m = m.clone();
                                crate::model::run_root(async move { m.request_ssid_permission().await });
                            })
                    },
                ))
        }))
}

/// A config in the menu: selects it; a subscription's row shows when it
/// updates and refreshes it; right-click copies its link or deletes it.
fn config_menu_row(m: Model, name: ReadSignal<String>) -> impl IntoElement {
    let selected = {
        let m = m.clone();
        move || m.selected_config_name().as_deref() == Some(name.get().as_str())
    };
    let is_remote = {
        let m = m.clone();
        move || m.subscription(&name.get()).is_some()
    };
    let subtitle = {
        let m = m.clone();
        move || {
            let _tick = m.remote_clock.get();
            let file = name.get();
            m.subscription(&file).and_then(|sub| crate::subscriptions::status_line(&m, &sub, m.config_modified_at(&file), crate::format::now_secs()))
        }
    };
    let phase = {
        let m = m.clone();
        move || m.remote_phases.with(|p| p.get(&name.get()).copied()).unwrap_or_default()
    };
    let (press_m, menu_m, refresh_m) = (m.clone(), m.clone(), m.clone());
    let subtitle_shown = {
        let subtitle = subtitle.clone();
        move || subtitle().is_some()
    };
    let phase2 = phase.clone();
    let selected_mark = selected.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .min_h(px(38.0))
        .py(px(3.0))
        .px(px(style::SPACE6))
        .rounded(px(style::ROW_RADIUS))
        .hover(|s| s.bg(style::fill(0.05)))
        .on_press(move || {
            let m = press_m.clone();
            let name = name.get_untracked();
            m.side_popover.set(None);
            crate::model::run_root(async move { m.select_config(name).await });
        })
        .on_context_menu(move |e: &ContextMenuEvent| {
            let file = name.get_untracked();
            let mut items = Vec::new();
            if menu_m.settings.with_untracked(|s| s.ssid_strategy_enabled) {
                use nsh_core::element::desktop::MenuItem;
                match menu_m.current_ssid.get_untracked() {
                    Some(ssid) if menu_m.ssid_authorization.get_untracked() == crate::wifi_types::Authorization::Authorized => {
                        let bound = menu_m.ssids_for_config(&file).contains(&ssid);
                        let key = if bound { "ui.quick.ssid.unbind_current_named" } else { "ui.quick.ssid.bind_current_named" };
                        let (m, f) = (menu_m.clone(), file.clone());
                        items.push(MenuItem::new(menu_m.tr_args(key, &[&ssid])).on_select(move || {
                            let m = m.clone();
                            let f = f.clone();
                            crate::model::run_root(async move { m.toggle_current_ssid_binding(f).await });
                        }));
                    }
                    _ => {
                        let m = menu_m.clone();
                        items.push(MenuItem::new(menu_m.tr("ui.quick.ssid.request_permission")).on_select(move || {
                            let m = m.clone();
                            crate::model::run_root(async move { m.request_ssid_permission().await });
                        }));
                    }
                }
                for ssid in menu_m.ssids_for_config(&file) {
                    if Some(&ssid) == menu_m.current_ssid.get_untracked().as_ref() {
                        continue;
                    }
                    let m = menu_m.clone();
                    let label = menu_m.tr_args("ui.quick.ssid.unbind_named", &[&ssid]);
                    items.push(MenuItem::new(label).on_select(move || m.remove_ssid_binding(&ssid)));
                }
                items.push(MenuItem::separator());
            }
            if menu_m.subscription(&file).is_some() {
                let (m, f) = (menu_m.clone(), file.clone());
                items.push(nsh_core::element::desktop::MenuItem::new(menu_m.tr("ui.action.copy_subscription_url")).on_select(move || m.copy_subscription_url(&f)));
            }
            {
                let (m, f) = (menu_m.clone(), file.clone());
                items.push(nsh_core::element::desktop::MenuItem::new(menu_m.tr("ui.action.delete")).destructive(true).on_select(move || {
                    let m = m.clone();
                    let f = f.clone();
                    m.side_popover.set(None);
                    crate::model::run_root(async move { m.delete_config(f).await });
                }));
            }
            let _ = nsh_core::element::desktop::popup_menu_at(items, e.page_position);
        })
        .child(dynamic({
            let is_remote = is_remote.clone();
            move || {
                if is_remote() {
                    widgets::tile(icons::link(), style::purple).into_element()
                } else {
                    widgets::tile(icons::doc_text(), style::info).into_element()
                }
            }
        }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(text(move || name.get()).truncate().text_size(px(font::BODY)).font_weight(FontWeight::MEDIUM).text_color(style::primary_label()))
                .child(when(subtitle_shown, move || {
                    let subtitle = subtitle.clone();
                    let phase = phase.clone();
                    text(move || subtitle().unwrap_or_default())
                        .truncate()
                        .text_size(px(font::CAPTION))
                        .text_color(move || if phase() == crate::subscriptions::RemotePhase::Failed { style::negative() } else { style::tertiary_label() })
                })),
        )
        .child({
            let m = m.clone();
            let bm = m.clone();
            let badge = move || {
                let ssids = m.ssids_for_config(&name.get());
                // ClashBar's `configMenuSSIDBadgeText`.
                let joined = ssids.join(", ");
                match ssids.len() {
                    0 => String::new(),
                    _ if joined.chars().count() <= 22 => joined,
                    1 => ssids[0].clone(),
                    n => format!("{} +{}", ssids[0], n - 1),
                }
            };
            let badge2 = badge.clone();
            when(move || !badge2().is_empty(), move || {
                let badge = badge.clone();
                let bm = bm.clone();
                text(move || badge())
                    .max_w(px(90.0))
                    .truncate()
                    .text_size(px(font::CAPTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || {
                        let current = bm.current_ssid.get();
                        if current.is_some_and(|c| bm.ssids_for_config(&name.get()).contains(&c)) { style::positive() } else { style::info() }
                    })
                    .px(px(style::SPACE4))
                    .rounded(px(6.0))
                    .bg(style::badge_fill())
            })
        })
        .child(when(selected_mark, || icon_view(icons::checkmark(), 13.0, style::info)))
        .child(when(is_remote, move || {
            let m = refresh_m.clone();
            let phase = phase2.clone();
            let label_m = m.clone();
            icon_button(
                move || match phase() {
                    crate::subscriptions::RemotePhase::Refreshing => icons::timer(),
                    crate::subscriptions::RemotePhase::Failed => icons::warning_triangle(),
                    crate::subscriptions::RemotePhase::Idle => icons::arrows_circlepath(),
                },
                style::secondary_label,
                move || label_m.tr("ui.action.refresh"),
                move || {
                    let m = m.clone();
                    let file = name.get_untracked();
                    crate::model::run_root(async move { m.refresh_remote_config(file).await });
                },
            )
        }))
}

/// The subscription import form (ClashBar's import alert's fields), as
/// the panel's cards: the link and file name; updating on its own; back to
/// the config menu, or import.
fn import_remote_form(m: Model) -> impl IntoElement {
    let url = signal(String::new());
    let file_name = signal(String::new());
    let auto_update = signal(false);
    let hours = signal(crate::settings::RemoteSubscription::DEFAULT_INTERVAL_HOURS.to_string());
    let missing_url = signal(false);
    let field = |placeholder: String, value: Signal<String>| {
        widgets::field_track()
            .flex_row()
            .items_center()
            .h(px(28.0))
            .child(
                input()
                    .flex_1()
                    .value(move || value.get())
                    .on_change(move |v: &str| value.set(v.to_owned()))
                    .placeholder(placeholder)
                    .text_size(px(font::BODY))
                    .text_color(style::primary_label()),
            )
    };
    let label = |m: &Model, key: &'static str| {
        let m = m.clone();
        text(move || m.tr(key)).text_size(px(font::CAPTION)).font_weight(FontWeight::SEMIBOLD).text_color(style::secondary_label())
    };
    let back_m = m.clone();
    let header = view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE4))
        .h(px(28.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(2.0))
                .px(px(style::SPACE2))
                .h(px(24.0))
                .rounded(px(6.0))
                .hover(|s| s.bg(style::fill(0.06)))
                .on_press(move || {
                    let anchor = back_m.side_popover.with_untracked(|p| p.as_ref().map_or(0.0, |p| p.anchor_mid_y));
                    back_m.side_measured.set(0.0);
                    back_m.side_popover.set(Some(SidePopover { kind: SideKind::Configs, anchor_mid_y: anchor }));
                })
                .child(icon_view(icons::chevron_left(), 11.0, style::info))
                .child(text({
                    let m = m.clone();
                    move || m.tr("ui.quick.remote.back")
                }).text_size(px(font::BODY)).text_color(style::info())),
        )
        .child(text({
            let m = m.clone();
            move || m.tr("ui.quick.import_remote_config")
        }).flex_1().text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::primary_label()));
    let link_card = widgets::card()
        .gap(px(style::SPACE4))
        .p(px(style::SPACE6))
        .child(label(&m, "ui.quick.remote.url_label"))
        .child(field(m.tr("ui.quick.remote.url_placeholder"), url))
        .child(when(move || missing_url.get() && url.with(|u| u.trim().is_empty()), {
            let m = m.clone();
            move || text(m.tr("ui.quick.remote.url_required")).text_size(px(font::CAPTION)).text_color(style::negative())
        }))
        .child(view().h(px(2.0)))
        .child(label(&m, "ui.quick.remote.filename_label"))
        .child(field(m.tr("ui.quick.remote.filename_placeholder"), file_name));
    let auto_m = m.clone();
    let hint_m = m.clone();
    let auto_card = widgets::card()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .px(px(style::SPACE6))
                .min_h(px(38.0))
                .on_press(move || auto_update.set(!auto_update.get_untracked()))
                .child(widgets::tile(icons::arrows_circlepath(), style::positive))
                .child(
                    view()
                        .flex_1()
                        .child(text(move || auto_m.tr("ui.quick.remote.auto_update_label")).text_size(px(font::BODY)).font_weight(FontWeight::MEDIUM).text_color(style::primary_label()))
                        .child(text(move || hint_m.tr("ui.quick.remote.auto_update_hint")).text_size(px(font::CAPTION)).text_color(style::tertiary_label())),
                )
                .child(widgets::switch(move || auto_update.get(), || false)),
        )
        .child(when(move || auto_update.get(), {
            let m = m.clone();
            move || {
                view()
                    .child(widgets::divider(ROW_INSET))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(style::SPACE4))
                            .pl(px(ROW_INSET))
                            .pr(px(style::SPACE6))
                            .min_h(px(34.0))
                            .child(label(&m, "ui.quick.remote.interval_each"))
                            .child(field(String::new(), hours).w(px(56.0)))
                            .child(label(&m, "ui.quick.remote.interval_unit")),
                    )
            }
        }));
    let cancel_m = m.clone();
    let import_m = m.clone();
    let can_import = move || !url.with(|u| u.trim().is_empty());
    let buttons = view()
        .flex_row()
        .gap(px(style::SPACE6))
        .child(
            view()
                .flex_1()
                .h(px(28.0))
                .items_center()
                .justify_center()
                .rounded(px(style::ROW_RADIUS))
                .bg(style::fill(0.07))
                .hover(|s| s.bg(style::fill(0.11)))
                .on_press(move || cancel_m.side_popover.set(None))
                .child(text({
                    let m = m.clone();
                    move || m.tr("ui.action.cancel")
                }).text_size(px(font::BODY)).text_color(style::primary_label())),
        )
        .child(
            view()
                .flex_1()
                .h(px(28.0))
                .items_center()
                .justify_center()
                .rounded(px(style::ROW_RADIUS))
                .bg(style::info())
                .opacity(move || if can_import() { 1.0_f32 } else { 0.45_f32 })
                .on_press(move || {
                    if url.with_untracked(|u| u.trim().is_empty()) {
                        missing_url.set(true);
                        return;
                    }
                    let m = import_m.clone();
                    let input = crate::subscriptions::ImportInput {
                        url: url.get_untracked().trim().to_owned(),
                        file_name: file_name.get_untracked(),
                        auto_update: auto_update.get_untracked(),
                        interval_hours: hours.get_untracked().trim().parse().unwrap_or(crate::settings::RemoteSubscription::DEFAULT_INTERVAL_HOURS),
                    };
                    m.side_popover.set(None);
                    crate::model::run_root(async move { m.import_remote_config(input).await });
                })
                .child(text({
                    let m = m.clone();
                    move || m.tr("ui.action.import")
                }).text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::white())),
        );
    view()
        .gap(px(style::SPACE6))
        .child(header)
        .child(text({
            let m = m.clone();
            move || m.tr("app.config.remote_import.prompt")
        }).px(px(style::SPACE2)).text_size(px(font::CAPTION)).text_color(style::secondary_label()))
        .child(link_card)
        .child(auto_card)
        .child(buttons)
}

/// The system proxy's quick row (ClashBar's `systemProxyQuickToggleRow`).
fn system_proxy_row(m: Model) -> impl IntoElement {
    let (on_m, dis_m, press_m, title_m, hint_m, hint2_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(style::SPACE6))
        .min_h(px(36.0))
        .rounded(px(style::ROW_RADIUS))
        .hover(|s| s.bg(style::fill(0.05)))
        .on_press(move || {
            // The system proxy points at this Mac's core.
            if !press_m.proxy_syncing.get_untracked() {
                let m = press_m.clone();
                let next = !m.system_proxy_enabled.get_untracked();
                crate::model::run_root(async move { m.toggle_system_proxy(next).await });
            }
        })
        .child(widgets::tile(icons::network(), style::info))
        .child(text(move || title_m.tr("ui.quick.system_proxy")).text_size(px(font::BODY)).font_weight(FontWeight::MEDIUM).text_color(style::primary_label()))
        .child(when(
            move || !hint_m.system_proxy_enabled.get() && hint_m.system_proxy_hint.with(|h| h.is_some()),
            move || {
                let m = hint2_m.clone();
                let tip_m = m.clone();
                view().flex_1().min_w(px(0.0)).tooltip(crate::widgets::tip(move || tip_m.system_proxy_hint.get().unwrap_or_default())).child(
                    text(move || m.system_proxy_hint.get().unwrap_or_default())
                        .truncate()
                        .text_size(px(font::CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(style::warning()),
                )
            },
        ))
        .child(view().flex_1())
        .child(crate::ui_settings::switch(move || on_m.system_proxy_enabled.get(), move || dis_m.proxy_syncing.get()))
}

/// The TUN quick row with its stack chips (ClashBar's `tunModeQuickToggleRow`).
fn tun_row(m: Model) -> impl IntoElement {
    let chips: Vec<_> = crate::features::TUN_STACKS
        .iter()
        .map(|&stack| {
            let (a1, a3, en, press, tip) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
            let active = move |m: &Model| m.tun_enabled() && m.tun_stack().as_deref() == Some(stack);
            text(stack)
                .text_size(px(font::CAPTION))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if active(&a1) { style::positive() } else { style::secondary_label() })
                .px(px(5.0))
                .py(px(2.0))
                .rounded(px(5.0))
                .bg(move || if active(&a3) { style::positive().opacity(0.18) } else { style::fill(0.0) })
                .opacity(move || {
                    let enabled = en.is_live() && (stack != "mips" || en.is_mips_supported());
                    if enabled { 1.0_f32 } else { 0.5_f32 }
                })
                .tooltip(crate::widgets::tip(move || {
                    if stack == "mips" && !tip.is_mips_supported() {
                        tip.tr_args("app.tun.mips_requires_upgrade", &[&crate::features::MIPS_MIN_CORE])
                    } else {
                        tip.tr("ui.quick.tun_stack_switch_hint")
                    }
                }))
                .on_press(move || {
                    if press.is_live_untracked() {
                        let m = press.clone();
                        crate::model::run_root(async move { m.select_tun_stack(stack).await });
                    }
                })
        })
        .collect();
    let (on_m, dis_m, toggle_m, title_m) = (m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(style::SPACE6))
        .min_h(px(36.0))
        .child(widgets::tile(icons::shield(), style::positive))
        .child(text(move || title_m.tr("ui.quick.tun_mode")).text_size(px(font::BODY)).font_weight(FontWeight::MEDIUM).text_color(style::primary_label()))
        .child(view().flex_1())
        .child(view().flex_row().gap(px(1.0)).p(px(2.0)).rounded(px(style::ROW_RADIUS)).bg(style::control_track()).children(chips))
        .child(
            view()
                .ml(px(style::SPACE4))
                .label({
                    let m = m.clone();
                    move || m.tr("ui.quick.tun_mode")
                })
                .on_press(move || {
                    if toggle_m.is_live_untracked() && !toggle_m.tun_syncing.get_untracked() {
                        let m = toggle_m.clone();
                        let next = !m.tun_enabled();
                        crate::model::run_root(async move { m.toggle_tun(next).await });
                    }
                })
                .child(crate::ui_settings::switch(move || on_m.tun_enabled(), move || !dis_m.is_live() || dis_m.tun_syncing.get())),
        )
}
