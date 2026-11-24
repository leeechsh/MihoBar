//! Proxy groups, providers and latency (ClashBar's `Proxy` / `Provider`
//! models and `AppViewModel+Proxy`, `+Providers`).

use std::collections::{BTreeMap, HashMap, HashSet};

use nsh_core::prelude::*;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::api::{self, Endpoint};
use crate::model::{CoreMode, Model};

/// Delays kept per proxy (ClashBar's `ProxyDelayHistory.limit`).
pub const DELAY_HISTORY_LIMIT: usize = 4;
pub const DEFAULT_HEALTHCHECK_URL: &str = "https://www.gstatic.com/generate_204";
pub const DEFAULT_HEALTHCHECK_TIMEOUT_MS: u32 = 5000;

fn flexible_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn trimmed_non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// The `history` array's delays, the last [`DELAY_HISTORY_LIMIT`].
fn delay_history<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i64>, D::Error> {
    let raw = Option::<Vec<Value>>::deserialize(d)?.unwrap_or_default();
    let mut samples: Vec<i64> = raw.iter().filter_map(|e| e.get("delay").and_then(flexible_int)).collect();
    if samples.len() > DELAY_HISTORY_LIMIT {
        samples.drain(..samples.len() - DELAY_HISTORY_LIMIT);
    }
    Ok(samples)
}

fn positive_int<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Option::<Value>::deserialize(d)?.as_ref().and_then(flexible_int).filter(|v| *v > 0))
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProxyGroup {
    #[serde(default = "unknown_name")]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub now: Option<String>,
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(rename = "testUrl", default)]
    pub test_url: Option<String>,
    #[serde(default, deserialize_with = "positive_int")]
    pub timeout: Option<i64>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(rename = "history", default, deserialize_with = "delay_history")]
    pub delay_history: Vec<i64>,
}

fn unknown_name() -> String {
    "Unknown".into()
}

#[derive(Deserialize)]
struct ProxiesResponse {
    proxies: HashMap<String, ProxyGroup>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Default)]
pub struct SubscriptionInfo {
    #[serde(alias = "Upload", default)]
    pub upload: Option<i64>,
    #[serde(alias = "Download", default)]
    pub download: Option<i64>,
    #[serde(alias = "Total", default)]
    pub total: Option<i64>,
    #[serde(alias = "Expire", default)]
    pub expire: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProviderNode {
    #[serde(default = "dash")]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(rename = "history", default, deserialize_with = "delay_history")]
    pub delay_history: Vec<i64>,
}

fn dash() -> String {
    "-".into()
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProviderDetail {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "vehicleType", default)]
    pub vehicle_type: Option<String>,
    #[serde(rename = "testUrl", default)]
    pub test_url: Option<String>,
    #[serde(default, deserialize_with = "positive_int")]
    pub timeout: Option<i64>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<String>,
    #[serde(rename = "ruleCount", alias = "rulesCount", alias = "count", default)]
    pub rule_count: Option<i64>,
    #[serde(rename = "subscriptionInfo", default)]
    pub subscription_info: Option<SubscriptionInfo>,
    #[serde(default)]
    pub proxies: Option<Vec<ProviderNode>>,
}

#[derive(Deserialize)]
struct ProviderSummary {
    providers: HashMap<String, ProviderDetail>,
}

/// A proxy provider shown: not mihomo's `default` and not `Compatible`.
fn include_provider(key: &str, detail: &ProviderDetail) -> bool {
    let name = trimmed_non_empty(detail.name.clone()).unwrap_or_else(|| key.to_owned());
    if name.eq_ignore_ascii_case("default") {
        return false;
    }
    !detail.vehicle_type.as_deref().unwrap_or("").trim().eq_ignore_ascii_case("Compatible")
}

/// A delay's color tier (ClashBar's `latencyColor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyTone {
    Unknown,
    Timeout,
    Good,
    Slow,
}

pub fn latency_tone(value: Option<i64>) -> LatencyTone {
    match value {
        None => LatencyTone::Unknown,
        Some(0) => LatencyTone::Timeout,
        Some(v) if v <= 400 => LatencyTone::Good,
        Some(_) => LatencyTone::Slow,
    }
}

fn available(latency: Option<i64>) -> bool {
    latency.is_some_and(|v| v > 0)
}

/// Names in order, empty ones and repeats dropped.
pub fn ordered_unique(names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    names.iter().filter(|n| !n.is_empty() && seen.insert(n.as_str())).cloned().collect()
}

/// Nodes by latency: available ones first, fastest first, then by name.
pub fn sort_by_latency(names: &[String], latency: impl Fn(&str) -> Option<i64>) -> Vec<String> {
    let mut nodes = ordered_unique(names);
    nodes.sort_by(|a, b| {
        let (la, lb) = (latency(a), latency(b));
        match (available(la), available(lb)) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) if la != lb => la.cmp(&lb),
            _ => natural_cmp(a, b),
        }
    });
    nodes
}

/// Names compared with their digit runs as numbers (`localizedStandardCompare`).
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ia, mut ib) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ia.peek().copied(), ib.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let na: String = std::iter::from_fn(|| ia.next_if(|c| c.is_ascii_digit())).collect();
                let nb: String = std::iter::from_fn(|| ib.next_if(|c| c.is_ascii_digit())).collect();
                let ord = na.trim_start_matches('0').len().cmp(&nb.trim_start_matches('0').len()).then(na.cmp(&nb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(ca), Some(cb)) => {
                let ord = ca.to_lowercase().cmp(cb.to_lowercase());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                ia.next();
                ib.next();
            }
        }
    }
}

impl Model {
    // --- Reading -----------------------------------------------------------

    /// Reads the groups and proxy providers (ClashBar's `refreshProxyGroups`).
    pub async fn refresh_proxy_groups(&self) -> bool {
        let target = self.target.get_untracked();
        let groups = api::request::<ProxiesResponse>(&target, &Endpoint::Proxies).await;
        let providers = api::request::<ProviderSummary>(&target, &Endpoint::ProxyProviders).await;
        let Ok(groups) = groups else { return false };
        let providers: BTreeMap<String, ProviderDetail> = match providers {
            Ok(p) => p.providers.into_iter().filter(|(k, d)| include_provider(k, d)).collect(),
            Err(e) => {
                self.log_app("info", self.tr_args("log.providers.api_unavailable", &[&e]));
                self.providers.get_untracked()
            }
        };
        self.apply_proxies(groups.proxies, &providers);
        if self.providers.get_untracked() != providers {
            self.providers.set(providers);
        }
        true
    }

    fn apply_proxies(&self, proxies: HashMap<String, ProxyGroup>, providers: &BTreeMap<String, ProviderDetail>) {
        // Groups in the order of GLOBAL's members, GLOBAL last, the rest by name.
        let mut order: HashMap<String, usize> = HashMap::new();
        let global_all = proxies.get("GLOBAL").map(|g| g.all.clone()).unwrap_or_default();
        for (i, name) in global_all.iter().chain(std::iter::once(&"GLOBAL".to_owned())).enumerate() {
            order.entry(name.clone()).or_insert(i);
        }
        let mut samples: HashMap<String, Vec<i64>> = HashMap::new();
        let mut types: HashMap<String, String> = HashMap::new();
        for p in proxies.values() {
            if p.all.is_empty() {
                if let Some(t) = trimmed_non_empty(p.kind.clone()) {
                    types.insert(p.name.clone(), t);
                }
            }
            if !p.delay_history.is_empty() {
                samples.insert(p.name.clone(), p.delay_history.clone());
            }
        }
        for provider in providers.values() {
            for node in provider.proxies.iter().flatten() {
                if !node.delay_history.is_empty() {
                    samples.entry(node.name.clone()).or_insert_with(|| node.delay_history.clone());
                }
                if let Some(t) = trimmed_non_empty(node.kind.clone()) {
                    types.entry(node.name.clone()).or_insert(t);
                }
            }
        }
        let mut groups: Vec<ProxyGroup> = proxies
            .into_values()
            .filter(|g| !g.all.is_empty())
            .map(|mut g| {
                let provider = providers.get(&g.name);
                g.test_url = trimmed_non_empty(g.test_url.take())
                    .or_else(|| provider.and_then(|p| trimmed_non_empty(p.test_url.clone())));
                g.timeout = g.timeout.or_else(|| provider.and_then(|p| p.timeout));
                g.icon = trimmed_non_empty(g.icon.take());
                g
            })
            .collect();
        groups.sort_by(|a, b| {
            let (oa, ob) = (order.get(&a.name).copied().unwrap_or(usize::MAX), order.get(&b.name).copied().unwrap_or(usize::MAX));
            oa.cmp(&ob).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        if self.groups.get_untracked() != groups {
            self.groups.set(groups);
        }
        // The API's histories win; delays measured here stay for proxies it
        // has none for (ClashBar's `mergeProxyDelaySamples`).
        let mut merged = samples;
        for (name, series) in self.delay_samples.get_untracked() {
            merged.entry(name).or_insert(series);
        }
        if self.delay_samples.get_untracked() != merged {
            self.delay_samples.set(merged);
        }
        if self.node_types.get_untracked() != types {
            self.node_types.set(types);
        }
    }

    /// The groups the panel lists: GLOBAL only in global mode, hidden ones
    /// only when asked (ClashBar's `filteredProxyGroups`).
    pub fn visible_groups(&self) -> Vec<ProxyGroup> {
        let global = self.mode.get() == Some(CoreMode::Global);
        let hide_hidden = self.settings.with(|s| s.hide_hidden_proxy_groups);
        self.groups.with(|groups| {
            groups
                .iter()
                .filter(|g| global || !g.name.eq_ignore_ascii_case("GLOBAL"))
                .filter(|g| !hide_hidden || g.hidden != Some(true))
                .cloned()
                .collect()
        })
    }

    pub fn clear_proxy_data(&self) {
        self.groups.set(Vec::new());
        self.providers.set(BTreeMap::new());
        self.group_latency_loading.set(HashSet::new());
        self.proxy_latency_testing.set(HashSet::new());
        self.provider_updating.set(HashSet::new());
    }

    // --- Delays ------------------------------------------------------------

    /// The group's selection chain from `start`: a group, the group it
    /// selects, and so on to a proxy.
    fn selection_chain(&self, start: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut current = start.to_owned();
        self.groups.with(|groups| {
            while seen.insert(current.clone()) {
                chain.push(current.clone());
                let Some(next) = groups
                    .iter()
                    .find(|g| g.name == current && !g.all.is_empty())
                    .and_then(|g| trimmed_non_empty(g.now.clone()))
                else {
                    break;
                };
                current = next;
            }
        });
        chain
    }

    /// The delays of `node` (followed to the proxy it selects), else, when
    /// asked, those of `group`'s selection.
    pub fn delay_samples_for(&self, group: &str, node: &str, fallback_to_group: bool) -> Vec<i64> {
        let lookup = |start: &str| {
            let chain = self.selection_chain(start);
            self.delay_samples.with(|samples| {
                chain.iter().rev().find_map(|n| samples.get(n).filter(|s| !s.is_empty()).cloned())
            })
        };
        lookup(node).or_else(|| fallback_to_group.then(|| lookup(group)).flatten()).unwrap_or_default()
    }

    pub fn delay_value(&self, group: &str, node: &str) -> Option<i64> {
        self.delay_samples_for(group, node, false).last().copied()
    }

    /// `123ms`, `超时`, or `--`.
    pub fn delay_text(&self, value: Option<i64>) -> String {
        match value {
            None => self.tr("ui.common.unknown"),
            Some(0) => self.tr("ui.common.timeout"),
            Some(v) => self.tr_args("ui.common.latency_ms", &[&v]),
        }
    }

    fn apply_measured_delays(&self, delays: HashMap<String, i64>) {
        if delays.is_empty() {
            return;
        }
        self.delay_samples.update(|samples| {
            for (name, delay) in delays {
                let series = samples.entry(name).or_default();
                series.push(delay);
                if series.len() > DELAY_HISTORY_LIMIT {
                    series.drain(..series.len() - DELAY_HISTORY_LIMIT);
                }
            }
        });
    }

    fn healthcheck_of(&self, group: &str) -> (String, u32) {
        self.groups.with_untracked(|groups| {
            let g = groups.iter().find(|g| g.name == group);
            let url = g.and_then(|g| g.test_url.clone()).unwrap_or_else(|| DEFAULT_HEALTHCHECK_URL.into());
            let timeout = g.and_then(|g| g.timeout).map(|t| t as u32).unwrap_or(DEFAULT_HEALTHCHECK_TIMEOUT_MS);
            (url, timeout)
        })
    }

    /// Tests the delays of a group's proxies (ClashBar's `refreshGroupLatency`).
    pub async fn refresh_group_latency(&self, group: String) {
        if self.group_latency_loading.with_untracked(|s| s.contains(&group)) {
            return;
        }
        self.group_latency_loading.update(|s| {
            s.insert(group.clone());
        });
        let (url, timeout) = self.healthcheck_of(&group);
        let target = self.target.get_untracked();
        let endpoint = Endpoint::GroupDelay { name: group.clone(), url, timeout };
        if let Ok(values) = api::request::<HashMap<String, Value>>(&target, &endpoint).await {
            let delays = values.into_iter().filter_map(|(k, v)| flexible_int(&v).map(|d| (k, d))).collect();
            self.apply_measured_delays(delays);
        }
        self.group_latency_loading.update(|s| {
            s.remove(&group);
        });
    }

    /// Tests every listed group at once (ClashBar's `refreshAllGroupLatencies`).
    pub async fn refresh_all_group_latencies(&self, include_hidden: bool) {
        let global = self.mode.get_untracked() == Some(CoreMode::Global);
        let names: Vec<String> = self.groups.with_untracked(|groups| {
            groups
                .iter()
                .filter(|g| global || !g.name.eq_ignore_ascii_case("GLOBAL"))
                .filter(|g| include_hidden || g.hidden != Some(true))
                .map(|g| g.name.clone())
                .collect()
        });
        let tasks: Vec<_> = names
            .into_iter()
            .map(|name| {
                let m = self.clone();
                spawn(async move { m.refresh_group_latency(name).await })
            })
            .collect();
        for task in tasks {
            task.await;
        }
    }

    /// Tests one proxy (ClashBar's `refreshProxyLatency`).
    pub async fn refresh_proxy_latency(&self, group: String, node: String) {
        let key = (group.clone(), node.clone());
        if self.proxy_latency_testing.with_untracked(|s| s.contains(&key)) {
            return;
        }
        self.proxy_latency_testing.update(|s| {
            s.insert(key.clone());
        });
        let (url, timeout) = self.healthcheck_of(&group);
        let target = self.target.get_untracked();
        let endpoint = Endpoint::ProxyDelay { name: node.clone(), url, timeout };
        if let Ok(v) = api::request::<Value>(&target, &endpoint).await {
            if let Some(delay) = v.get("delay").and_then(flexible_int) {
                self.apply_measured_delays(HashMap::from([(node, delay)]));
            }
        }
        self.proxy_latency_testing.update(|s| {
            s.remove(&key);
        });
    }

    // --- Actions -----------------------------------------------------------

    /// Selects `target` in `group` (ClashBar's `switchProxy`).
    pub async fn switch_proxy(&self, group: String, target_node: String) {
        let action = self.tr_args("log.action_name.switch_proxy", &[&group, &target_node]);
        let target = self.target.get_untracked();
        let endpoint = Endpoint::SwitchProxy { name: group, target: target_node };
        match api::request_no_response(&target, &endpoint).await {
            Ok(()) => {
                self.log_app("info", self.tr_args("log.action.success", &[&action]));
                self.refresh_proxy_groups().await;
            }
            Err(e) => self.log_app("error", self.tr_args("log.action.failed", &[&action, &e])),
        }
    }

    /// Updates a proxy provider (ClashBar's `updateProxyProvider`).
    pub async fn update_proxy_provider(&self, name: String) {
        if self.provider_updating.with_untracked(|s| s.contains(&name)) {
            return;
        }
        self.provider_updating.update(|s| {
            s.insert(name.clone());
        });
        let action = self.tr_args("log.action_name.update_proxy_provider", &[&name]);
        let target = self.target.get_untracked();
        match api::request_no_response(&target, &Endpoint::UpdateProxyProvider(name.clone())).await {
            Ok(()) => {
                self.log_app("info", self.tr_args("log.action.success", &[&action]));
                self.refresh_proxy_groups().await;
            }
            Err(e) => self.log_app("error", self.tr_args("log.action.failed", &[&action, &e])),
        }
        self.provider_updating.update(|s| {
            s.remove(&name);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_groups_and_history() {
        let json = r#"{"name":"G","type":"Selector","now":"a","all":["a","b"],"timeout":"0",
            "history":[{"delay":1},{"delay":"2"},{"delay":3},{"delay":4},{"delay":5}]}"#;
        let g: ProxyGroup = serde_json::from_str(json).unwrap();
        assert_eq!(g.delay_history, vec![2, 3, 4, 5]);
        assert_eq!(g.timeout, None);
        let info: SubscriptionInfo = serde_json::from_str(r#"{"Upload":1,"Download":2,"Total":3,"Expire":0}"#).unwrap();
        assert_eq!(info.total, Some(3));
    }

    #[test]
    fn sorts_by_latency() {
        let names: Vec<String> = ["n10", "n2", "slow", "dead", "n2"].iter().map(|s| s.to_string()).collect();
        let lat = |n: &str| match n {
            "n10" => Some(80),
            "n2" => Some(80),
            "slow" => Some(500),
            "dead" => Some(0),
            _ => None,
        };
        assert_eq!(sort_by_latency(&names, lat), vec!["n2", "n10", "slow", "dead"]);
        assert_eq!(latency_tone(Some(401)), LatencyTone::Slow);
    }
}
