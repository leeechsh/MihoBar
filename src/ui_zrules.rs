//! The advanced window's rules page (zashboard's): the core's rules with
//! their rule sets' sizes, hit counts, a switch to disable each, and the
//! proxy chain each leads to (the group, its selected node, the latency);
//! the rule providers with their updates. A regex searches either list.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nsh_core::element::desktop::{MenuItem, popup_menu};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use serde_json::Value;

use crate::api::{self, Endpoint};
use crate::icons;
use crate::model::Model;
use crate::style::z;
use crate::ui_zconns::{icon_button, search_field, segment, segments, sheet};

/// A rule as the core reports it (`/rules`).
#[derive(Clone, Debug, PartialEq)]
pub struct ZRule {
    pub index: i64,
    pub kind: String,
    pub payload: String,
    pub proxy: String,
    /// The rule set's rules (-1: not a rule set).
    pub size: i64,
    pub disabled: bool,
    pub hits: i64,
    pub hit_at: String,
    pub misses: i64,
    pub miss_at: String,
}

impl ZRule {
    fn from_value(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        let extra = v.get("extra");
        let e = |k: &str| extra.and_then(|x| x.get(k));
        Some(Self {
            index: v.get("index").and_then(Value::as_i64)?,
            kind: s("type"),
            payload: s("payload"),
            proxy: s("proxy"),
            size: v.get("size").and_then(Value::as_i64).unwrap_or(-1),
            disabled: e("disabled").and_then(Value::as_bool).unwrap_or(false),
            hits: e("hitCount").and_then(Value::as_i64).unwrap_or(0),
            hit_at: e("hitAt").and_then(Value::as_str).unwrap_or_default().to_owned(),
            misses: e("missCount").and_then(Value::as_i64).unwrap_or(0),
            miss_at: e("missAt").and_then(Value::as_str).unwrap_or_default().to_owned(),
        })
    }
}

/// A rule provider (`/providers/rules`).
#[derive(Clone, Debug, PartialEq)]
pub struct ZProvider {
    pub name: String,
    pub behavior: String,
    pub vehicle: String,
    pub count: i64,
    pub updated_at: String,
}

/// What the chain needs of a proxy: its selected node (a group) and its
/// last latency.
#[derive(Clone, Debug, PartialEq, Default)]
struct ProxyInfo {
    now: Option<String>,
    all: Vec<String>,
    delay: i64,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Rules,
    Providers,
}

#[derive(Clone, Copy)]
struct State {
    tab: Signal<Tab>,
    search: Signal<String>,
    rules: Signal<Rc<Vec<ZRule>>>,
    providers: Signal<Rc<Vec<ZProvider>>>,
    proxies: Signal<Rc<HashMap<String, ProxyInfo>>>,
    updating: Signal<HashSet<String>>,
    settings_open: Signal<bool>,
}

async fn load(m: &Model, state: State) {
    let target = m.target.get_untracked();
    if let Ok(v) = api::request::<Value>(&target, &Endpoint::Rules).await {
        let rules: Vec<ZRule> = v.get("rules").and_then(Value::as_array).map(|a| a.iter().filter_map(ZRule::from_value).collect()).unwrap_or_default();
        state.rules.set(Rc::new(rules));
    }
    if let Ok(v) = api::request::<Value>(&target, &Endpoint::RuleProviders).await {
        let mut list: Vec<ZProvider> = v
            .get("providers")
            .and_then(Value::as_object)
            .map(|o| {
                o.values()
                    .map(|p| {
                        let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
                        ZProvider { name: s("name"), behavior: s("behavior"), vehicle: s("vehicleType"), count: p.get("ruleCount").and_then(Value::as_i64).unwrap_or(0), updated_at: s("updatedAt") }
                    })
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        state.providers.set(Rc::new(list));
    }
    load_proxies(m, state).await;
}

async fn load_proxies(m: &Model, state: State) {
    let target = m.target.get_untracked();
    if let Ok(v) = api::request::<Value>(&target, &Endpoint::Proxies).await {
        let map: HashMap<String, ProxyInfo> = v
            .get("proxies")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(name, p)| {
                        let delay = p.get("history").and_then(Value::as_array).and_then(|h| h.last()).and_then(|x| x.get("delay")).and_then(Value::as_i64).unwrap_or(0);
                        let all = p.get("all").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
                        (name.clone(), ProxyInfo { now: p.get("now").and_then(Value::as_str).map(str::to_owned).filter(|s| !s.is_empty()), all, delay })
                    })
                    .collect()
            })
            .unwrap_or_default();
        state.proxies.set(Rc::new(map));
    }
}

pub fn rules_page(m: Model) -> impl IntoElement {
    let state = State {
        tab: signal(Tab::Rules),
        search: signal(String::new()),
        rules: signal(Rc::new(Vec::new())),
        providers: signal(Rc::new(Vec::new())),
        proxies: signal(Rc::new(HashMap::new())),
        updating: signal(HashSet::new()),
        settings_open: signal(false),
    };
    {
        let m = m.clone();
        spawn(async move { load(&m, state).await });
    }
    view()
        .flex_1()
        .pt(px(30.0))
        .child(controls(m.clone(), state))
        .child(dynamic({
            let m = m.clone();
            move || match state.tab.get() {
                Tab::Rules => rule_list(m.clone(), state).into_element(),
                Tab::Providers => provider_list(m.clone(), state).into_element(),
            }
        }))
        .child(when(move || state.settings_open.get(), move || settings_sheet(m.clone(), state)))
}

fn update_provider(m: &Model, state: State, name: String) {
    let m = m.clone();
    state.updating.update(|u| {
        u.insert(name.clone());
    });
    spawn(async move {
        let target = m.target.get_untracked();
        let _ = api::request_no_response(&target, &Endpoint::UpdateRuleProvider(name.clone())).await;
        state.updating.update(|u| {
            u.remove(&name);
        });
        load(&m, state).await;
    });
}

fn controls(m: Model, state: State) -> impl IntoElement {
    let rules_m = m.clone();
    let prov_m = m.clone();
    let tabs = segments(vec![
        segment(move || format!("{} {}", rules_m.tr("z.rules"), state.rules.with(|r| r.len())), move || state.tab.get() == Tab::Rules, move || state.tab.set(Tab::Rules)).into_element(),
        segment(move || format!("{} {}", prov_m.tr("z.rule_provider"), state.providers.with(|p| p.len())), move || state.tab.get() == Tab::Providers, move || state.tab.set(Tab::Providers)).into_element(),
    ]);
    let all_m = m.clone();
    let all_l = m.clone();
    let settings_l = m.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .pb(px(8.0))
        .child(tabs)
        .child(search_field(m.tr("z.search_regex"), state.search, view().into_element(), 260.0))
        .child(when(move || state.tab.get() == Tab::Providers, move || {
            let m = all_m.clone();
            let l = all_l.clone();
            icon_button(icons::z_refresh, move || l.tr("z.update_all"), move || {
                let names: Vec<String> = state.providers.with_untracked(|p| p.iter().filter(|x| x.vehicle != "Inline").map(|x| x.name.clone()).collect());
                for name in names {
                    update_provider(&m, state, name);
                }
            })
        }))
        .child(view().flex_1())
        .child(icon_button(icons::z_tools, move || settings_l.tr("z.rule_settings"), move || state.settings_open.set(true)))
}

fn list_card(list: impl IntoElement) -> impl IntoElement {
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
        .child(list)
}

fn rule_list(m: Model, state: State) -> impl IntoElement {
    let shown = computed(move || {
        let re = crate::conns::search_regex(&state.search.get());
        state.rules.with(|rules| {
            rules
                .iter()
                .filter(|r| re.as_ref().is_none_or(|re| [&r.kind, &r.payload, &r.proxy].iter().any(|t| re.is_match(t))))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let list = virtual_list(move || shown.with(Vec::len), move |at: nsh_core::element::Position| {
        let item = computed(move || shown.with(|l| l.get(at.index()).cloned()));
        let m = m.clone();
        view().child(dynamic(move || {
            // Made again as the providers (sizes), proxies (chains) or the
            // rule settings come or change.
            state.providers.with(|_| ());
            state.proxies.with(|_| ());
            m.z_settings.with(|s| (s.rule_show_now, s.rule_show_latency, s.rule_disconnect_on_disable));
            match item.get() {
                Some(rule) => untracked(|| rule_row(m.clone(), state, rule)).into_element(),
                None => view().into_element(),
            }
        }))
    })
    .estimated_item_size(px(64.0))
    .flex_1();
    list_card(list)
}

fn latency_color(ms: i64) -> AdaptiveColor {
    match ms {
        ms if ms < 400 => z::success(),
        ms if ms < 800 => z::warning(),
        _ => z::error(),
    }
}

/// A chip of the chain (zashboard's `ProxyChainPath`): pressing a group
/// lists its nodes to choose from.
fn chain_chip(m: Model, state: State, name: String, latency: Option<i64>) -> impl IntoElement {
    let group = state.proxies.with_untracked(|p| p.get(&name).filter(|i| !i.all.is_empty()).cloned());
    let target_name = name.clone();
    let mut chip = view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(22.0))
        .px(px(8.0))
        .rounded(px(11.0))
        .bg(z::base200(1.0))
        .child(text(name).text_size(px(12.0)).text_color(z::text(0.85)))
        .children(latency.filter(|l| *l > 0).map(|l| text(l.to_string()).text_size(px(11.0)).text_color(latency_color(l))));
    if let Some(info) = group {
        chip = chip.hover(|s| s.bg(z::base300(0.6))).on_press(move || {
            let current = info.now.clone();
            let items = info
                .all
                .iter()
                .map(|node| {
                    let (m, group, node) = (m.clone(), target_name.clone(), node.clone());
                    MenuItem::new(node.clone()).checked(current.as_deref() == Some(node.as_str())).on_select(move || {
                        let (m, group, node) = (m.clone(), group.clone(), node.clone());
                        spawn(async move {
                            let target = m.target.get_untracked();
                            let _ = api::request_no_response(&target, &Endpoint::SwitchProxy { name: group, target: node }).await;
                            load_proxies(&m, state).await;
                        });
                    })
                })
                .collect();
            let _ = popup_menu(items);
        });
    }
    chip
}

fn rule_row(m: Model, state: State, rule: ZRule) -> impl IntoElement {
    let size = if rule.size >= 0 {
        Some(rule.size)
    } else if rule.kind == "RuleSet" {
        state.providers.with_untracked(|p| p.iter().find(|x| x.name == rule.payload).map(|x| x.count))
    } else {
        None
    };
    let updatable = rule.kind == "RuleSet" && state.providers.with_untracked(|p| p.iter().any(|x| x.name == rule.payload && x.vehicle != "Inline"));
    let (show_now, show_latency, disconnect) = m.z_settings.with_untracked(|s| (s.rule_show_now, s.rule_show_latency, s.rule_disconnect_on_disable));
    // The chain: the rule's proxy, then (shown) each group's selected node.
    let mut chain = vec![rule.proxy.clone()];
    if show_now {
        let proxies = state.proxies.get_untracked();
        let mut seen = HashSet::new();
        while let Some(next) = proxies.get(chain.last().unwrap()).and_then(|p| p.now.clone()) {
            if !seen.insert(next.clone()) {
                break;
            }
            chain.push(next);
        }
    }
    let leaf_delay = if show_latency { state.proxies.with_untracked(|p| p.get(chain.last().unwrap()).map(|i| i.delay)) } else { None };
    let n = chain.len();
    let mut path = view().flex_row().items_center().gap(px(6.0));
    for (i, name) in chain.into_iter().enumerate() {
        path = path.child(chain_chip(m.clone(), state, name, if i + 1 == n { leaf_delay } else { None }));
        if i + 1 < n {
            path = path.child(image(ImageSrc::from(icons::z_chevron_small())).w(px(11.0)).h(px(11.0)).tint_color(z::text(0.4)));
        }
    }
    let tip = format!(
        "{} {}  {}\n{} {}  {}",
        m.tr("z.hits"),
        rule.hits,
        crate::format::from_now(crate::format::parse_iso8601_ms(&rule.hit_at).unwrap_or(0).max(1), crate::format::now_secs() * 1000, m.is_zh()),
        m.tr("z.misses"),
        rule.misses,
        crate::format::from_now(crate::format::parse_iso8601_ms(&rule.miss_at).unwrap_or(0).max(1), crate::format::now_secs() * 1000, m.is_zh()),
    );
    let provider = rule.payload.clone();
    let updating = {
        let p = provider.clone();
        move || state.updating.with(|u| u.contains(&p))
    };
    let update_m = m.clone();
    let toggle_m = m.clone();
    let r2 = rule.clone();
    let mut head = view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .min_h(px(20.0))
        .child(text(rule.index.to_string()).w(px(28.0)).text_size(px(12.0)).text_color(z::text(0.5)))
        .child(text(if rule.payload.is_empty() { rule.kind.clone() } else { format!("{} :", rule.kind) }).text_size(px(12.0)).text_color(z::text(0.55)))
        .children((!rule.payload.is_empty()).then(|| text(rule.payload.clone()).text_size(px(14.0)).text_color(z::text(1.0))))
        .children(size.map(|s| text(format!("({s})")).text_size(px(12.0)).text_color(z::text(0.5))));
    if updatable {
        head = head.child(
            view()
                .w(px(22.0))
                .h(px(22.0))
                .items_center()
                .justify_center()
                .rounded(px(11.0))
                .hover(|s| s.bg(z::text(0.07)))
                .on_press(move || update_provider(&update_m, state, provider.clone()))
                .child(image(ImageSrc::from(icons::z_refresh())).w(px(13.0)).h(px(13.0)).tint_color(z::text(0.6)).opacity(move || if updating() { 0.3_f32 } else { 1.0_f32 })),
        );
    }
    head = head.child(view().tooltip(crate::widgets::tip(tip)).child(image(ImageSrc::from(icons::z_info())).w(px(14.0)).h(px(14.0)).tint_color(z::text(0.5))));
    view()
        .gap(px(6.0))
        .px(px(12.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(z::base300(0.3))
        .opacity(if rule.disabled { 0.5 } else { 1.0 })
        .hover(|s| s.bg(z::base200(0.4)))
        .child(head)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .pl(px(34.0))
                .child(
                    view()
                        .on_press(move || {
                            let m = toggle_m.clone();
                            let rule = r2.clone();
                            spawn(async move {
                                let disable = !rule.disabled;
                                let target = m.target.get_untracked();
                                let body = serde_json::json!({ rule.index.to_string(): disable });
                                if api::request_no_response(&target, &Endpoint::DisableRules(body)).await.is_ok() {
                                    if disable && disconnect {
                                        // Its connections (zashboard's disconnectOnRuleDisable).
                                        let ids: Vec<String> = m.z_active.with_untracked(|a| {
                                            a.iter()
                                                .filter(|c| {
                                                    c.raw.0.get("rule").and_then(Value::as_str) == Some(rule.kind.as_str())
                                                        && c.raw.0.get("rulePayload").and_then(Value::as_str).unwrap_or("") == rule.payload
                                                })
                                                .map(|c| c.id.clone())
                                                .collect()
                                        });
                                        for id in ids {
                                            let _ = api::request_no_response(&target, &Endpoint::CloseConnection(id)).await;
                                        }
                                    }
                                    load(&m, state).await;
                                }
                            });
                        })
                        .child(crate::widgets::switch(move || !rule.disabled, || false)),
                )
                .child(path),
        )
}

fn provider_list(m: Model, state: State) -> impl IntoElement {
    let shown = computed(move || {
        let re = crate::conns::search_regex(&state.search.get());
        state.providers.with(|list| {
            list.iter()
                .filter(|p| re.as_ref().is_none_or(|re| [&p.name, &p.behavior, &p.vehicle].iter().any(|t| re.is_match(t))))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let list = virtual_list(move || shown.with(Vec::len), move |at: nsh_core::element::Position| {
        let item = computed(move || shown.with(|l| l.get(at.index()).cloned()));
        let m = m.clone();
        view().child(dynamic(move || match item.get() {
            Some(p) => provider_row(m.clone(), state, p, at.index() + 1).into_element(),
            None => view().into_element(),
        }))
    })
    .estimated_item_size(px(44.0))
    .flex_1();
    list_card(list)
}

fn provider_row(m: Model, state: State, p: ZProvider, index: usize) -> impl IntoElement {
    let meta = [p.behavior.clone(), p.vehicle.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let updated = format!(
        "{} {}",
        m.tr("z.updated"),
        crate::format::from_now(crate::format::parse_iso8601_ms(&p.updated_at).unwrap_or(0), crate::format::now_secs() * 1000, m.is_zh())
    );
    let name = p.name.clone();
    let updating = {
        let n = name.clone();
        move || state.updating.with(|u| u.contains(&n))
    };
    let update_m = m.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(16.0))
        .px(px(16.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(z::base300(0.3))
        .hover(|s| s.bg(z::base200(0.4)))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(text(index.to_string()).text_size(px(12.0)).text_color(z::text(0.5)))
                .child(text(p.name.clone()).truncate().text_size(px(14.0)).text_color(z::text(1.0)))
                .child(text(format!("· {} {}", p.count, m.tr("z.rules_word"))).text_size(px(12.0)).text_color(z::text(0.5))),
        )
        .child(text(meta).text_size(px(12.0)).text_color(z::text(0.5)))
        .child(text(updated).text_size(px(12.0)).text_color(z::text(0.5)))
        .children((p.vehicle != "Inline").then(|| {
            view()
                .w(px(24.0))
                .h(px(24.0))
                .items_center()
                .justify_center()
                .rounded(px(12.0))
                .hover(|s| s.bg(z::text(0.07)))
                .on_press(move || update_provider(&update_m, state, name.clone()))
                .child(image(ImageSrc::from(icons::z_refresh())).w(px(13.0)).h(px(13.0)).tint_color(z::text(0.6)).opacity(move || if updating() { 0.3_f32 } else { 1.0_f32 }))
        }))
}

fn settings_sheet(m: Model, state: State) -> impl IntoElement {
    let toggle = |label: String, get: fn(&crate::conns::AdvancedSettings) -> bool, set: fn(&mut crate::conns::AdvancedSettings)| {
        let (g, s) = (m.clone(), m.clone());
        view()
            .flex_row()
            .items_center()
            .min_h(px(44.0))
            .child(text(label).flex_1().text_size(px(13.0)).text_color(z::text(1.0)))
            .child(view().on_press(move || s.z_settings.update(set)).child(crate::widgets::switch(move || g.z_settings.with(get), || false)))
    };
    let body = view()
        .px(px(20.0))
        .py(px(12.0))
        .child(toggle(m.tr("z.display_selected_node"), |s| s.rule_show_now, |s| s.rule_show_now = !s.rule_show_now))
        .child(toggle(m.tr("z.display_latency_number"), |s| s.rule_show_latency, |s| s.rule_show_latency = !s.rule_show_latency))
        .child(toggle(m.tr("z.disconnect_on_rule_disable"), |s| s.rule_disconnect_on_disable, |s| s.rule_disconnect_on_disable = !s.rule_disconnect_on_disable));
    sheet(m.tr("z.rule_settings"), 480.0, 260.0, move || state.settings_open.set(false), Vec::new(), body)
}
