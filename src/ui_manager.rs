//! The advanced window, after zashboard (leeechsh's fork): a narrow
//! sidebar of pages with the live figures under it, and the pages —
//! connections, usage, logs, rules. In the app bundle it is a process of
//! its own (`AdvancedLaunch`), started by the panel's and ended as the
//! window closes, so all it used goes back to the system; outside one
//! (development, tests) it is a window of the panel's process. Only the
//! current page is mounted, and the lists and streams behind a page live as
//! long as it shows.

use std::rc::Rc;

use nsh_core::element::ImageSrc;
use nsh_core::element::desktop::{MenuItem, popup_menu};
use nsh_core::element::input;
use nsh_core::prelude::*;

use crate::format::{pretty_bytes, pretty_bytes_parts};
use crate::icons;
use crate::model::{ManagerSection, Model};
use crate::style::z;
use crate::usage::{self, Aggregate, Dimension, Record, UsageConfig};

pub const WIDTH: f32 = 1180.0;
pub const HEIGHT: f32 = 780.0;
pub const MIN_WIDTH: f32 = 980.0;
pub const MIN_HEIGHT: f32 = 600.0;
/// zashboard's collapsed sidebar (`w-18`).
const RAIL: f32 = 72.0;

fn section_icon(section: ManagerSection) -> String {
    match section {
        ManagerSection::Connections => icons::z_connections(),
        ManagerSection::Usage => icons::z_usage(),
        ManagerSection::Logs => icons::z_logs(),
        ManagerSection::Rules => icons::z_rules(),
    }
}

fn glyph(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> nsh_core::element::Image {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

/// zashboard's `base-container`: a white rounded card.
fn card() -> View {
    view().bg(z::card()).rounded(px(12.0)).border_1().border_color(z::base300(0.35)).shadow_xs()
}

/// The window's content.
pub fn manager(m: Model) -> impl IntoElement {
    // The top strip moves the window (its title bar is hidden).
    let drag = view().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(28.0)).window_drag(true);
    view().flex_1().flex_row().bg(z::page()).child(rail(m.clone())).child(content(m)).child(drag)
}

// --- Sidebar --------------------------------------------------------------------------

fn rail(m: Model) -> impl IntoElement {
    let items: Vec<_> = ManagerSection::ALL.iter().map(|&s| rail_item(m.clone(), s)).collect();
    view()
        .w(px(RAIL))
        .pt(px(40.0))
        .pb(px(8.0))
        .items_center()
        .gap(px(6.0))
        .bg(z::page())
        .border_r_1()
        .border_color(z::base300(0.3))
        .children(items)
        .child(view().flex_1())
        .child(rail_stats(m))
}

fn rail_item(m: Model, section: ManagerSection) -> impl IntoElement {
    let selected = {
        let m = m.clone();
        move || m.manager_section.get() == section
    };
    let (s1, s2) = (selected.clone(), selected);
    let press_m = m.clone();
    view()
        .w(px(44.0))
        .h(px(40.0))
        .items_center()
        .justify_center()
        .rounded(px(10.0))
        .bg(move || if s1() { z::card() } else { z::text(0.0) })
        .hover(|s| s.bg(z::text(0.05)))
        .transition_colors()
        .duration(150.0)
        .tooltip(crate::widgets::tip({
            let m = m.clone();
            move || m.tr(section.title_key())
        }))
        .label(move || m.tr(section.title_key()))
        .on_press(move || press_m.manager_section.set(section))
        .child(glyph(section_icon(section), 20.0, move || if s2() { z::text(1.0) } else { z::text(0.6) }))
}

/// One figure of the sidebar: an icon, the value, its unit and a total.
fn stat_cell(icon: String, value: impl Fn() -> (String, String) + Clone + 'static, total: Option<Box<dyn Fn() -> String>>) -> View {
    let unit = value.clone();
    view()
        .w_full()
        .items_center()
        .gap(px(3.0))
        .py(px(7.0))
        .border_t_1()
        .border_color(z::base300(0.35))
        .child(glyph(icon, 13.0, || z::text(0.7)))
        .child(text(move || value().0).text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(z::text(1.0)))
        .child(text(move || unit().1).text_size(px(9.0)).text_color(z::text(0.55)))
        .children(total.map(|t| text(move || t()).text_size(px(9.0)).text_color(z::text(0.4))))
}

/// The figures under the pages (zashboard's `VerticalInfos`): connections,
/// the speeds with their totals, memory.
fn rail_stats(m: Model) -> impl IntoElement {
    let (c_m, d_m, dt_m, u_m, ut_m, mem_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    let speed = |v: i64| {
        let (n, u) = pretty_bytes_parts(v.max(0) as u64);
        (n, format!("{u}/s"))
    };
    view()
        .w(px(60.0))
        .rounded(px(10.0))
        .bg(z::card())
        .border_1()
        .border_color(z::base300(0.35))
        .overflow_hidden()
        .child(
            view()
                .w_full()
                .items_center()
                .gap(px(3.0))
                .py(px(7.0))
                .child(glyph(icons::z_connections(), 13.0, || z::text(0.7)))
                .child(text(move || c_m.connections_count.get().to_string()).text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(z::text(1.0))),
        )
        .child(stat_cell(icons::z_arrow_down(), move || speed(d_m.traffic.get().down), Some(Box::new(move || pretty_bytes(dt_m.connection_totals.get().1)))))
        .child(stat_cell(icons::z_arrow_up(), move || speed(u_m.traffic.get().up), Some(Box::new(move || pretty_bytes(ut_m.connection_totals.get().0)))))
        .child(stat_cell(icons::z_memory(), move || pretty_bytes_parts(mem_m.memory.get().max(0) as u64), None))
}

// --- Pages ------------------------------------------------------------------------------

/// The current page. Each is made as it is chosen and dropped as another is.
fn content(m: Model) -> impl IntoElement {
    let shown = |m: &Model, section: ManagerSection| {
        let m = m.clone();
        move || m.manager_section.get() == section
    };
    let (rules_m, conn_m, logs_m, usage_m) = (m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_1()
        .min_w(px(0.0))
        .child(when(shown(&m, ManagerSection::Connections), move || crate::ui_zconns::connections_page(conn_m.clone())))
        .child(when(shown(&m, ManagerSection::Usage), move || usage_page(usage_m.clone())))
        .child(when(shown(&m, ManagerSection::Logs), move || crate::ui_zlogs::logs_page(logs_m.clone())))
        .child(when(shown(&m, ManagerSection::Rules), move || crate::ui_zrules::rules_page(rules_m.clone())))
}

// --- Usage --------------------------------------------------------------------------------

fn dimension_label(m: &Model, d: Dimension) -> String {
    m.tr(match d {
        Dimension::SourceIp => "z.devices",
        Dimension::InboundUser => "z.inbound_user",
        Dimension::Host => "z.host",
        Dimension::Outbound => "z.outbound",
        Dimension::Process => "z.process",
    })
}

fn dimension_icon(d: Dimension) -> String {
    match d {
        Dimension::SourceIp => icons::z_devices(),
        Dimension::InboundUser => icons::z_user(),
        Dimension::Host => icons::z_globe(),
        Dimension::Outbound => icons::z_connections(),
        Dimension::Process => icons::z_cpu(),
    }
}

/// The ranges (zashboard: the last hour, day, week, month).
const RANGES: [(i64, &str); 4] =
    [(3600, "z.last_hour"), (86400, "z.last_day"), (7 * 86400, "z.last_week"), (30 * 86400, "z.last_month")];
/// How long usage is kept (zashboard's retention; a month by default here).
const RETENTIONS: [(i64, &str); 5] =
    [(-1, "z.forever"), (3600, "z.last_hour"), (86400, "z.last_day"), (7 * 86400, "z.last_week"), (30 * 86400, "z.last_month")];

#[derive(Clone, Copy)]
struct UsageState {
    dimension: Signal<Dimension>,
    range: Signal<i64>,
    config: Signal<UsageConfig>,
    search: Signal<String>,
    selected: Signal<Option<String>>,
    expanded: Signal<Option<String>>,
    records: Signal<Rc<Vec<Record>>>,
    end: Signal<i64>,
    rows: Memo<Vec<Aggregate>>,
    detail_rows: Memo<Vec<Aggregate>>,
}

impl UsageState {
    /// What the detail table lists: the dimension, or, with a top entry
    /// chosen, its devices (for a host) or hosts.
    fn detail_dimension(&self) -> Dimension {
        match self.selected.get() {
            None => self.dimension.get(),
            Some(_) if self.dimension.get() == Dimension::Host => Dimension::SourceIp,
            Some(_) => Dimension::Host,
        }
    }

    /// The rows under an expanded detail row (zashboard's `childRows`).
    fn child_rows(&self, label: &str) -> Vec<Aggregate> {
        let dimension = self.dimension.get_untracked();
        let selected = self.selected.get_untracked();
        let detail = match &selected {
            None => dimension,
            Some(_) if dimension == Dimension::Host => Dimension::SourceIp,
            Some(_) => Dimension::Host,
        };
        let child = match (&selected, dimension) {
            (Some(_), Dimension::Outbound) => Dimension::SourceIp,
            (Some(_), _) => Dimension::Outbound,
            (None, Dimension::Outbound) => Dimension::Host,
            (None, _) => Dimension::Outbound,
        };
        self.records.with_untracked(|records| {
            let mut rows = usage::aggregate(
                records
                    .iter()
                    .filter(|r| r.get(detail) == label && selected.as_deref().is_none_or(|s| r.get(dimension) == s)),
                child,
            );
            rows.truncate(12);
            rows
        })
    }
}

fn usage_page(m: Model) -> impl IntoElement {
    let dir = m.usage_dir();
    let dimension = signal(Dimension::Host);
    let records: Signal<Rc<Vec<Record>>> = signal(Rc::new(Vec::new()));
    let selected: Signal<Option<String>> = signal(None);
    let rows = computed(move || records.with(|r| usage::aggregate(r.iter(), dimension.get())));
    let detail_rows = computed(move || match selected.get() {
        None => rows.get(),
        Some(sel) => {
            let d = dimension.get();
            let detail = if d == Dimension::Host { Dimension::SourceIp } else { Dimension::Host };
            records.with(|r| usage::aggregate(r.iter().filter(|x| x.get(d) == sel), detail))
        }
    });
    let state = UsageState {
        dimension,
        range: signal(3600),
        config: signal(UsageConfig::load(&dir)),
        search: signal(String::new()),
        selected,
        expanded: signal(None),
        records,
        end: signal(crate::format::now_secs()),
        rows,
        detail_rows,
    };

    // Read again as the dimension or range changes, and every 30 s (as the
    // menu bar process writes).
    let refresh: Rc<dyn Fn()> = Rc::new({
        let dir = dir.clone();
        move || {
            let end = crate::format::now_secs();
            let start = end - state.range.get_untracked();
            let dir = dir.clone();
            spawn(async move {
                let loaded = blocking(move || usage::load(&dir, start, end)).await.unwrap_or_default();
                state.end.set(end);
                state.records.set(Rc::new(loaded));
                if let Some(sel) = state.selected.get_untracked() {
                    if !state.rows.with_untracked(|r| r.iter().any(|a| a.label == sel)) {
                        state.selected.set(None);
                    }
                }
            });
        }
    });
    {
        let refresh = refresh.clone();
        effect(move || {
            let _ = (state.dimension.get(), state.range.get());
            untracked(|| refresh());
        });
    }
    {
        let refresh = refresh.clone();
        spawn(async move {
            loop {
                timer(std::time::Duration::from_secs(30)).await;
                refresh();
            }
        });
    }

    scroll().flex_1().scrollbar(false).child(
        view()
            .w_full()
            .p(px(12.0))
            .pt(px(36.0))
            .gap(px(12.0))
            .child(usage_toolbar(m.clone(), state, refresh, dir))
            .child(usage_summary(m.clone(), state))
            .child(view().flex_row().gap(px(12.0)).child(usage_top(m.clone(), state)).child(usage_chart(m.clone(), state)))
            .child(usage_details(m, state)),
    )
}

/// A select (zashboard's `select-sm`): the current option; a native menu.
fn select(label: impl Fn() -> String + 'static, items: impl Fn() -> Vec<MenuItem> + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(32.0))
        .px(px(12.0))
        .min_w(px(112.0))
        .rounded(px(8.0))
        .bg(z::base200(0.7))
        .border_1()
        .border_color(z::base300(0.6))
        .on_press(move || {
            let _ = popup_menu(items());
        })
        .child(text(label).flex_1().text_size(px(13.0)).text_color(z::text(1.0)))
        .child(glyph(icons::z_chevron_down(), 11.0, || z::text(0.6)))
}

fn usage_toolbar(m: Model, state: UsageState, refresh: Rc<dyn Fn()>, dir: std::path::PathBuf) -> impl IntoElement {
    let tabs: Vec<_> = Dimension::ALL
        .iter()
        .map(|&d| {
            let on = move || state.dimension.get() == d;
            let label_m = m.clone();
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .h(px(34.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .bg(move || if on() { z::primary(1.0) } else { z::primary(0.0) })
                .hover(|s| s.bg(z::base300(0.45)))
                .transition_colors()
                .duration(150.0)
                .on_press(move || {
                    state.dimension.set(d);
                    state.selected.set(None);
                    state.expanded.set(None);
                })
                .child(glyph(dimension_icon(d), 14.0, move || if on() { z::primary_content() } else { z::text(0.65) }))
                .child(
                    text(move || dimension_label(&label_m, d))
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if on() { z::primary_content() } else { z::text(0.65) }),
                )
        })
        .collect();
    let segmented = view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .p(px(4.0))
        .rounded(px(12.0))
        .bg(z::base200(0.7))
        .border_1()
        .border_color(z::base300(0.6))
        .children(tabs);

    let range_m = m.clone();
    let range_items_m = m.clone();
    let range = select(
        move || range_m.tr(RANGES.iter().find(|(v, _)| *v == state.range.get()).map_or("z.last_hour", |(_, k)| *k)),
        move || {
            RANGES
                .iter()
                .map(|&(v, k)| MenuItem::new(range_items_m.tr(k)).checked(state.range.get_untracked() == v).on_select(move || state.range.set(v)))
                .collect()
        },
    );
    // The settings: tracking, how long usage is kept, clearing it (in a
    // menu, so the toolbar fits a 1000 pt window, a narrow display's).
    let settings_m = m.clone();
    let settings_label_m = m.clone();
    let settings_dir = dir.clone();
    let settings_refresh = refresh.clone();
    let settings = crate::ui_zconns::icon_button(icons::z_tools, move || settings_label_m.tr("z.usage_settings"), move || {
        let m = settings_m.clone();
        let config = state.config.get_untracked();
        let track_dir = settings_dir.clone();
        let tracking = MenuItem::new(m.tr("z.tracking")).checked(config.tracking).on_select(move || {
            state.config.update(|c| c.tracking = !c.tracking);
            state.config.get_untracked().save(&track_dir);
        });
        let keep: Vec<MenuItem> = RETENTIONS
            .iter()
            .map(|&(v, k)| {
                let dir = settings_dir.clone();
                MenuItem::new(m.tr(k)).checked(config.retention_secs == v).on_select(move || {
                    state.config.update(|c| c.retention_secs = v);
                    state.config.get_untracked().save(&dir);
                })
            })
            .collect();
        let kept = RETENTIONS.iter().find(|(v, _)| *v == config.retention_secs).map_or("z.last_month", |(_, k)| *k);
        let keep = MenuItem::submenu(format!("{} · {}", m.tr("z.keep_data"), m.tr(kept)), keep);
        let (clear_m, clear_dir, clear_refresh) = (m.clone(), settings_dir.clone(), settings_refresh.clone());
        let clear = MenuItem::new(format!("{}…", m.tr("z.clear_usage"))).destructive(true).on_select(move || {
            let (m, dir, refresh) = (clear_m.clone(), clear_dir.clone(), clear_refresh.clone());
            crate::model::run_root(async move {
                let answer = nsh_base::alert_dialog()
                    .title(m.tr("z.confirm_clear_usage"))
                    .destructive("clear", m.tr("z.clear_usage"))
                    .cancel("cancel", m.tr("z.cancel"))
                    .show()
                    .await;
                if format!("{answer:?}").contains("clear") {
                    usage::clear(&dir);
                    state.config.update(|c| c.cleared_at = crate::format::now_secs());
                    state.config.get_untracked().save(&dir);
                    refresh();
                }
            });
        });
        let _ = popup_menu(vec![tracking, keep, MenuItem::separator(), clear]);
    });
    // Tracking off shows: usage is not being recorded.
    let paused_m = m.clone();
    let paused = when(
        move || !state.config.get().tracking,
        move || {
            text(paused_m.tr("z.tracking_off"))
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(z::warning())
                .px(px(8.0))
                .py(px(3.0))
                .rounded(px(6.0))
                .bg(z::warning().opacity(0.12))
        },
    );
    let refresh_m = m.clone();
    card()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(segmented)
        .child(view().flex_1().min_w(px(0.0)))
        .child(paused)
        .child(range)
        .child(crate::ui_zconns::icon_button(icons::z_refresh, move || refresh_m.tr("z.refresh"), move || refresh()))
        .child(settings)
}

fn summary_tile(icon: String, label: impl Fn() -> String + 'static, value: impl Fn() -> String + 'static) -> impl IntoElement {
    card()
        .flex_1()
        .min_w(px(0.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .h(px(82.0))
        .px(px(16.0))
        .child(
            view()
                .w(px(40.0))
                .h(px(40.0))
                .items_center()
                .justify_center()
                .rounded(px(12.0))
                .bg(z::primary(0.1))
                .child(glyph(icon, 20.0, || z::primary(1.0))),
        )
        .child(
            view()
                .min_w(px(0.0))
                .child(text(label).text_size(px(13.0)).text_color(z::text(0.6)))
                .child(text(value).text_size(px(20.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(1.0))),
        )
}

fn usage_summary(m: Model, state: UsageState) -> impl IntoElement {
    let totals = move || state.rows.with(|r| r.iter().fold((0u64, 0u64), |(u, d), a| (u + a.upload, d + a.download)));
    let (m1, m2, m3, m4) = (m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .gap(px(12.0))
        .child(summary_tile(icons::z_hashtag(), move || dimension_label(&m1, state.dimension.get()), move || state.rows.with(|r| r.len()).to_string()))
        .child(summary_tile(icons::z_cloud_up(), move || m2.tr("z.upload"), move || pretty_bytes(totals().0)))
        .child(summary_tile(icons::z_cloud_down(), move || m3.tr("z.download"), move || pretty_bytes(totals().1)))
        .child(summary_tile(icons::z_server(), move || m4.tr("z.total"), move || {
            let (u, d) = totals();
            pretty_bytes(u + d)
        }))
}

fn usage_top(m: Model, state: UsageState) -> impl IntoElement {
    let title_m = m.clone();
    let empty_m = m.clone();
    let top = move || state.rows.with(|r| r.iter().take(12).cloned().collect::<Vec<_>>());
    card()
        .flex_1()
        .min_w(px(0.0))
        .h(px(320.0))
        .p(px(12.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .mb(px(12.0))
                .child(glyph(icons::z_usage(), 16.0, || z::primary(1.0)))
                .child(
                    text(move || format!("{} · {}", title_m.tr("z.top_usage"), dimension_label(&title_m, state.dimension.get())))
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(z::text(0.8)),
                ),
        )
        .child(when(
            move || state.rows.with(|r| r.is_empty()),
            move || text(empty_m.tr("z.no_data")).px(px(12.0)).py(px(20.0)).text_size(px(13.0)).text_color(z::text(0.5)),
        ))
        .child(scroll().flex_1().scrollbar(false).child(view().gap(px(4.0)).child(each(top, |a: &Aggregate| a.label.clone(), move |row, _| {
            let label = move || row.with(|r| r.label.clone());
            let on = move || state.selected.with(|s| s.as_deref() == Some(row.with(|r| r.label.clone()).as_str()));
            view()
                .gap(px(4.0))
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(12.0))
                .bg(move || if on() { z::primary(0.12) } else { z::primary(0.0) })
                .hover(|s| s.bg(z::base200(1.0)))
                .on_press(move || {
                    let l = label();
                    state.selected.update(|s| *s = if s.as_deref() == Some(l.as_str()) { None } else { Some(l) });
                    state.expanded.set(None);
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(move || row.with(|r| r.label.clone()))
                                .flex_1()
                                .truncate()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || if on() { z::primary(1.0) } else { z::text(1.0) }),
                        )
                        .child(
                            text(move || pretty_bytes(row.with(|r| r.total())))
                                .text_size(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || if on() { z::primary(1.0) } else { z::text(1.0) }),
                        ),
                )
                .child(
                    text(move || row.with(|r| format!("↑ {}   ↓ {}", pretty_bytes(r.upload), pretty_bytes(r.download))))
                        .text_size(px(13.0))
                        .text_color(z::text(0.6)),
                )
        }))))
}

/// A "nice" step for about `ticks` divisions of `max` (1, 2 or 5 × 10ⁿ).
fn nice_step(max: f64, ticks: f64) -> f64 {
    let raw = (max / ticks).max(1.0);
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let nice = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

fn rgba_of(hex: u32, alpha: f32) -> Hsla {
    Rgba { r: ((hex >> 16) & 0xff) as f32 / 255.0, g: ((hex >> 8) & 0xff) as f32 / 255.0, b: (hex & 0xff) as f32 / 255.0, a: alpha }.into()
}

/// The traffic chart (zashboard's `TimeSeriesChart`): upload and download
/// per bucket over the range, as filled areas, with the byte scale on the
/// left and times under it.
fn usage_chart(m: Model, state: UsageState) -> impl IntoElement {
    let size = signal((600.0_f32, 240.0_f32));
    let data = move || {
        let end = state.end.get();
        let range = state.range.get();
        let bucket = usage::chart_bucket(range);
        state.records.with(|r| usage::series(r, end - range, end, bucket))
    };
    let scale = move || {
        let max = data().iter().map(|(_, u, d)| (*u).max(*d)).max().unwrap_or(0) as f64;
        let step = nice_step(max.max(1.0), 3.0);
        let top = (max / step).ceil().max(1.0) * step;
        (step, top)
    };
    let plot = canvas(move || {
        let (w, h) = size.get();
        let points = data();
        let (step, top) = scale();
        let n = points.len().max(2);
        let x = move |i: usize| i as f32 / (n - 1) as f32 * w;
        let y = move |v: f64| h - (v / top) as f32 * h;
        let mut painter = Painter::new();
        let mut level = 0.0;
        while level <= top + 0.5 {
            let gy = y(level);
            painter = painter.begin_path().move_to(0.0, gy).line_to(w, gy).stroke_color(rgba_of(0x8e8e93, 0.25)).line_width(0.6).stroke();
            level += step;
        }
        // Upload a light gray, download the theme's primary gray.
        // Mid grays that read on both themes (zashboard's chart is gray).
        for (hex, pick) in [(0xb0b0b5_u32, 1usize), (0x7c7c80, 2)] {
            if points.is_empty() {
                continue;
            }
            let value = |p: &(i64, u64, u64)| if pick == 1 { p.1 } else { p.2 } as f64;
            let line: String = points
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}{:.1} {:.1}", if i == 0 { "M" } else { "L" }, x(i), y(value(p))))
                .collect();
            let fill = format!("{line}L{:.1} {h:.1}L0 {h:.1}Z", x(points.len() - 1));
            let gradient = nsh_core::canvas::CanvasGradient::linear(point(px(0.0), px(0.0)), point(px(0.0), px(h)))
                .add_color_stop(percentage(0.0), rgba_of(hex, 0.45))
                .add_color_stop(percentage(1.0), rgba_of(hex, 0.03));
            painter = painter.fill_style(gradient).fill_svg(&fill).stroke_color(rgba_of(hex, 0.9)).line_width(1.2).stroke_svg(&line);
        }
        painter
    })
    .absolute()
    .top(px(0.0))
    .left(px(0.0))
    .right(px(0.0))
    .bottom(px(0.0));
    let y_labels = move || {
        let (step, top) = scale();
        let (_, h) = size.get();
        let mut out = Vec::new();
        let mut level = step;
        while level <= top + 0.5 {
            out.push((h - (level / top) as f32 * h, pretty_bytes(level as u64)));
            level += step;
        }
        out
    };
    let x_labels = move || {
        let points = data();
        let (w, _) = size.get();
        let n = points.len();
        if n < 2 {
            return Vec::new();
        }
        let count = 12.min(n);
        let range = state.range.get();
        (0..count)
            .map(|k| {
                let i = k * (n - 1) / (count - 1).max(1);
                let p = crate::format::local_parts(points[i].0);
                let label = if range <= 86400 { format!("{:02}:{:02}", p.3, p.4) } else { format!("{:02}-{:02}", p.1, p.2) };
                // Centered under its point, kept inside the plot.
                ((i as f32 / (n - 1) as f32 * w - 16.0).clamp(0.0, (w - 40.0).max(0.0)), label)
            })
            .collect::<Vec<_>>()
    };
    let legend = |hex: u32, label: String| {
        view()
            .flex_row()
            .items_center()
            .gap(px(4.0))
            .child(view().w(px(7.0)).h(px(7.0)).rounded(px(4.0)).bg(rgba_of(hex, 1.0)))
            .child(text(label).text_size(px(12.0)).text_color(z::text(0.7)))
    };
    card()
        .flex_grow(3.0)
        .basis(px(0.0))
        .min_w(px(0.0))
        .h(px(320.0))
        .p(px(12.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .child(text(m.tr("z.traffic")).flex_1().text_size(px(15.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(0.8)))
                .child(legend(0xb0b0b5, m.tr("z.upload")))
                .child(view().w(px(12.0)))
                .child(legend(0x7c7c80, m.tr("z.download"))),
        )
        .child(
            view()
                .flex_1()
                .flex_row()
                .mt(px(12.0))
                .child(view().w(px(56.0)).relative().child(dynamic(move || {
                    view().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).bottom(px(0.0)).children(
                        y_labels()
                            .into_iter()
                            .map(|(top, label)| text(label).absolute().top(px((top - 7.0).max(0.0))).right(px(8.0)).text_size(px(10.0)).text_color(z::text(0.5)))
                            .collect::<Vec<_>>(),
                    )
                })))
                .child(
                    view()
                        .flex_1()
                        .child(view().flex_1().relative().on_layout(move |b: &Bounds<Points>| size.set((b.size.width.0, b.size.height.0))).child(plot))
                        .child(view().h(px(20.0)).relative().child(dynamic(move || {
                            view().absolute().top(px(4.0)).left(px(0.0)).right(px(0.0)).h(px(14.0)).children(
                                x_labels()
                                    .into_iter()
                                    .map(|(left, label)| text(label).absolute().left(px(left)).w(px(40.0)).text_size(px(10.0)).text_color(z::text(0.5)))
                                    .collect::<Vec<_>>(),
                            )
                        }))),
                ),
        )
}

fn number_cell(value: impl Fn() -> String + 'static, strong: bool) -> impl IntoElement {
    text(value)
        .w(px(130.0))
        .text_right()
        .text_size(px(13.0))
        .font_weight(if strong { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
        .text_color(if strong { z::primary(1.0) } else { z::text(1.0) })
}

fn head_cell(label: String) -> impl IntoElement {
    text(label).w(px(130.0)).text_right().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(0.75))
}

fn usage_details(m: Model, state: UsageState) -> impl IntoElement {
    let detail_label = {
        let m = m.clone();
        move || dimension_label(&m, state.detail_dimension())
    };
    let filtered = move || {
        let q = state.search.get().trim().to_lowercase();
        state.detail_rows.with(|rows| rows.iter().filter(|r| q.is_empty() || r.label.to_lowercase().contains(&q)).take(300).cloned().collect::<Vec<_>>())
    };
    let counts = move || {
        let q = state.search.get().trim().to_lowercase();
        state.detail_rows.with(|rows| (rows.iter().filter(|r| q.is_empty() || r.label.to_lowercase().contains(&q)).count(), rows.len()))
    };
    let header_label = detail_label.clone();
    let title_m = m.clone();
    let row_m = m.clone();
    let expand_m = m.clone();
    card()
        .overflow_hidden()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(20.0))
                .py(px(14.0))
                .bg(z::base200(0.55))
                .border_b_1()
                .border_color(z::base300(0.6))
                .child(glyph(icons::z_bars(), 18.0, || z::primary(1.0)))
                .child(
                    text(move || format!("{} · {}", title_m.tr("z.usage_details"), detail_label()))
                        .flex_1()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(z::text(1.0)),
                )
                .child(
                    view()
                        .w(px(256.0))
                        .h(px(32.0))
                        .px(px(10.0))
                        .justify_center()
                        .rounded(px(8.0))
                        .bg(z::card())
                        .border_1()
                        .border_color(z::base300(0.7))
                        .child(
                            input()
                                .value(move || state.search.get())
                                .on_change(move |v: &str| state.search.set(v.to_owned()))
                                .placeholder(m.tr("z.search"))
                                .text_size(px(13.0))
                                .text_color(z::text(1.0)),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .h(px(46.0))
                .px(px(20.0))
                .bg(z::base200(0.8))
                .child(text(move || header_label()).flex_1().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::text(0.75)))
                .child(head_cell(row_m.tr("z.upload")))
                .child(head_cell(row_m.tr("z.download")))
                .child(head_cell(row_m.tr("z.total"))),
        )
        .child(view().child(each(filtered, |a: &Aggregate| a.label.clone(), move |row, index| {
            let label = move || row.with(|r| r.label.clone());
            let open = move || state.expanded.with(|e| e.as_deref() == Some(row.with(|r| r.label.clone()).as_str()));
            let expand_m = expand_m.clone();
            view()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .h(px(52.0))
                        .px(px(20.0))
                        .bg(move || {
                            if open() {
                                z::primary(0.05)
                            } else if index.get() % 2 == 1 {
                                z::base200(0.48)
                            } else {
                                z::card()
                            }
                        })
                        .hover(|s| s.bg(z::base200(0.9)))
                        .on_press(move || {
                            let l = label();
                            state.expanded.update(|e| *e = if e.as_deref() == Some(l.as_str()) { None } else { Some(l) });
                        })
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    image(move || ImageSrc::from(if open() { icons::z_chevron_down() } else { icons::z_chevron_right() }))
                                        .w(px(12.0))
                                        .h(px(12.0))
                                        .tint_color(z::primary(1.0)),
                                )
                                .child(text(move || row.with(|r| r.label.clone())).truncate().text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(z::text(1.0))),
                        )
                        .child(number_cell(move || pretty_bytes(row.with(|r| r.upload)), false))
                        .child(number_cell(move || pretty_bytes(row.with(|r| r.download)), false))
                        .child(number_cell(move || pretty_bytes(row.with(|r| r.total())), true)),
                )
                .child(when(open, move || expanded_row(expand_m.clone(), state, row)))
        })))
        .child(
            text(move || {
                let (shown, all) = counts();
                format!("{shown} / {all}")
            })
            .px(px(20.0))
            .py(px(12.0))
            .border_t_1()
            .border_color(z::base300(0.6))
            .text_size(px(13.0))
            .text_color(z::text(0.6)),
        )
}

/// An expanded detail row: its figures and, under them, its top entries
/// by another dimension.
fn expanded_row(m: Model, state: UsageState, row: ReadSignal<Aggregate>) -> impl IntoElement {
    let fact = |label: String, value: String| {
        view()
            .flex_1()
            .min_w(px(0.0))
            .child(text(label).text_size(px(13.0)).text_color(z::text(0.6)))
            .child(text(value).mt(px(4.0)).truncate().text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(z::text(1.0)))
    };
    let end = state.end.get_untracked();
    let start = end - state.range.get_untracked();
    let fmt = |t: i64| {
        let p = crate::format::local_parts(t);
        format!("{:04}-{:02}-{:02} {:02}:{:02}", p.0, p.1, p.2, p.3, p.4)
    };
    let end_text = fmt(end);
    let range = format!("{} – {}", fmt(start), &end_text[11..]);
    let children = state.child_rows(&row.with_untracked(|r| r.label.clone()));
    let child_views: Vec<_> = children
        .into_iter()
        .map(|c| {
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .min_h(px(48.0))
                .px(px(16.0))
                .py(px(10.0))
                .rounded(px(12.0))
                .bg(z::card())
                .border_1()
                .border_color(z::base300(0.6))
                .shadow_xs()
                .child(text(c.label.clone()).flex_1().truncate().text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(z::text(1.0)))
                .child(text(format!("↑ {} · ↓ {}", pretty_bytes(c.upload), pretty_bytes(c.download))).text_size(px(13.0)).text_color(z::text(0.65)))
                .child(text(pretty_bytes(c.total())).text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).text_color(z::primary(1.0)))
        })
        .collect();
    view()
        .gap(px(12.0))
        .px(px(20.0))
        .py(px(16.0))
        .bg(z::base200(0.7))
        .child(
            view()
                .flex_row()
                .gap(px(16.0))
                .child(fact(m.tr("z.records"), row.with_untracked(|r| r.count.to_string())))
                .child(fact(m.tr("z.range"), range))
                .child(fact(m.tr("z.backend"), m.target.get_untracked().controller))
                .child(fact(m.tr("z.collection"), m.tr("z.collection.local"))),
        )
        .children(child_views)
}
