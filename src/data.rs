//! Connections, logs and rules (ClashBar's `Connection`, `Log`, `Rules`
//! models, `AppLogStore`, `AppViewModel+Logging` and the connections,
//! logs and rules parts of `+Proxy`, `+Providers`, `+Streams`).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;

use serde::Deserialize;
use serde_json::Value;

/// Connections kept from a snapshot (ClashBar's `retainedConnectionLimit`).
pub const RETAINED_CONNECTIONS: usize = 120;
/// Rules kept (ClashBar's `retainedRuleLimit`).
pub const RETAINED_RULES: usize = 20000;
/// Logs kept while the panel shows, and while it is hidden.
pub const MAX_LOGS_VISIBLE: usize = 5000;
pub const MAX_LOGS_HIDDEN: usize = 20;
/// Rows a tab lists at most (ClashBar's `prefix(120)`).
pub const LISTED_ROWS: usize = 120;

fn string_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn first_string(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| obj.get(*k).and_then(string_of))
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ConnectionMetadata {
    pub network: Option<String>,
    pub source_ip: Option<String>,
    pub destination_ip: Option<String>,
    pub host: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub id: String,
    pub upload: i64,
    pub download: i64,
    pub start: Option<String>,
    pub start_timestamp: Option<i64>,
    pub rule: Option<String>,
    pub rule_payload: Option<String>,
    pub chains: Vec<String>,
    pub metadata: ConnectionMetadata,
}

impl Connection {
    fn from_value(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        let id = obj.get("id").and_then(string_of)?;
        let meta = obj.get("metadata").and_then(Value::as_object);
        let metadata = meta.map_or_else(ConnectionMetadata::default, |m| ConnectionMetadata {
            network: first_string(m, &["network", "networkType", "type"]),
            source_ip: first_string(m, &["sourceIP", "source_ip", "source", "clientIP"]),
            destination_ip: first_string(m, &["destinationIP", "destination_ip", "destination", "remoteIP", "remoteAddress"]),
            host: first_string(m, &["host", "destinationHost", "remoteHost", "addr"]),
        });
        let chains = ["chains", "chain"]
            .iter()
            .find_map(|k| match obj.get(*k) {
                Some(Value::Array(a)) => {
                    let list: Vec<String> = a.iter().filter_map(string_of).collect();
                    (!list.is_empty()).then_some(list)
                }
                Some(other) => string_of(other).map(|s| vec![s]),
                None => None,
            })
            .unwrap_or_default();
        let start = obj.get("start").and_then(string_of);
        Some(Self {
            id,
            upload: obj.get("upload").and_then(Value::as_i64).unwrap_or(0),
            download: obj.get("download").and_then(Value::as_i64).unwrap_or(0),
            start_timestamp: start.as_deref().and_then(crate::format::parse_iso8601),
            start,
            rule: obj.get("rule").and_then(string_of),
            rule_payload: first_string(obj, &["rulePayload", "rule_payload", "rulepayload", "payload"]),
            chains,
            metadata,
        })
    }

    /// The host, else the destination IP (ClashBar's `resolvedConnectionHost`).
    pub fn resolved_host(&self) -> Option<String> {
        self.metadata.host.clone().or_else(|| self.metadata.destination_ip.clone())
    }

    /// The chain from the first hop to the proxy (the API lists it reversed).
    pub fn chain_parts(&self) -> Vec<String> {
        self.chains.iter().rev().cloned().collect()
    }

    /// What the filter field searches (ClashBar's `connectionSearchText`).
    pub fn search_text(&self) -> String {
        let m = &self.metadata;
        [
            m.host.as_deref().unwrap_or(""),
            m.destination_ip.as_deref().unwrap_or(""),
            m.source_ip.as_deref().unwrap_or(""),
            m.network.as_deref().unwrap_or(""),
            &self.id,
            self.rule.as_deref().unwrap_or(""),
            self.rule_payload.as_deref().unwrap_or(""),
            &self.chain_parts().join(" > "),
            self.start.as_deref().unwrap_or(""),
        ]
        .join(" ")
    }
}

/// A snapshot's connections (the first 120) and how many it had.
pub fn parse_connections(payload: &str) -> Option<(Vec<Connection>, usize)> {
    #[derive(Deserialize)]
    struct Snapshot {
        #[serde(default)]
        connections: Option<Vec<Value>>,
    }
    let s: Snapshot = serde_json::from_str(payload).ok()?;
    let all = s.connections.unwrap_or_default();
    let total = all.len();
    Some((all.iter().take(RETAINED_CONNECTIONS).filter_map(Connection::from_value).collect(), total))
}

/// A rule's type and payload from its `rule` text: `Type(payload)` or
/// `Type,payload` (ClashBar's `parseConnectionRule`).
pub fn parse_rule(raw: Option<&str>) -> Option<(String, Option<String>)> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let non_empty = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
    if let (Some(open), Some(close)) = (raw.find('('), raw.rfind(')')) {
        if open < close {
            if let Some(kind) = non_empty(&raw[..open]) {
                return Some((kind, non_empty(&raw[open + 1..close])));
            }
        }
    }
    if let Some((kind, payload)) = raw.split_once(',') {
        if let Some(kind) = non_empty(kind) {
            return Some((kind, non_empty(payload)));
        }
    }
    Some((raw.to_owned(), None))
}

/// The rule type a row shows: `--` for MATCH / FINAL.
pub fn rule_type_text(raw: Option<&str>, parsed: Option<&str>) -> String {
    let candidate = parsed.or(raw).map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
    if candidate.is_empty() || candidate.eq_ignore_ascii_case("MATCH") || candidate.eq_ignore_ascii_case("FINAL") {
        "--".into()
    } else {
        candidate.to_owned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TransportFilter {
    #[default]
    All,
    Tcp,
    Udp,
    Other,
}

impl TransportFilter {
    pub const ALL: [TransportFilter; 4] = [Self::All, Self::Tcp, Self::Udp, Self::Other];
    pub fn title_key(self) -> &'static str {
        match self {
            Self::All => "ui.network.filter.transport.all",
            Self::Tcp => "ui.network.filter.transport.tcp",
            Self::Udp => "ui.network.filter.transport.udp",
            Self::Other => "ui.network.filter.transport.other",
        }
    }
    pub fn matches(self, network: Option<&str>) -> bool {
        let n = network.unwrap_or("").trim().to_lowercase();
        match self {
            Self::All => true,
            Self::Tcp => n == "tcp",
            Self::Udp => n == "udp",
            Self::Other => !n.is_empty() && n != "tcp" && n != "udp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionSort {
    #[default]
    Default,
    Newest,
    Oldest,
    UploadDesc,
    DownloadDesc,
    TotalDesc,
}

impl ConnectionSort {
    pub const ALL: [ConnectionSort; 6] =
        [Self::Default, Self::Newest, Self::Oldest, Self::UploadDesc, Self::DownloadDesc, Self::TotalDesc];
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Default => "ui.network.sort.default",
            Self::Newest => "ui.network.sort.newest",
            Self::Oldest => "ui.network.sort.oldest",
            Self::UploadDesc => "ui.network.sort.upload_desc",
            Self::DownloadDesc => "ui.network.sort.download_desc",
            Self::TotalDesc => "ui.network.sort.total_desc",
        }
    }
}

/// The connections the tab lists (ClashBar's `updateVisibleConnections`).
pub fn visible_connections(all: &[Connection], keyword: &str, transport: TransportFilter, sort: ConnectionSort) -> Vec<Connection> {
    let keyword = keyword.trim().to_lowercase();
    let mut list: Vec<Connection> = all
        .iter()
        .take(LISTED_ROWS)
        .filter(|c| transport.matches(c.metadata.network.as_deref()))
        .filter(|c| keyword.is_empty() || c.search_text().to_lowercase().contains(&keyword))
        .cloned()
        .collect();
    let by_time = |list: &mut Vec<Connection>, descending: bool| {
        let fallback = if descending { i64::MIN } else { i64::MAX };
        list.sort_by(|a, b| {
            let (l, r) = (a.start_timestamp.unwrap_or(fallback), b.start_timestamp.unwrap_or(fallback));
            if l != r {
                if descending { r.cmp(&l) } else { l.cmp(&r) }
            } else {
                crate::proxy::natural_cmp(&a.id, &b.id)
            }
        });
    };
    let by_traffic = |list: &mut Vec<Connection>, metric: fn(&Connection) -> i64| {
        list.sort_by(|a, b| {
            metric(b).cmp(&metric(a)).then_with(|| b.start_timestamp.unwrap_or(-1).cmp(&a.start_timestamp.unwrap_or(-1)))
        });
    };
    match sort {
        ConnectionSort::Default => {}
        ConnectionSort::Newest => by_time(&mut list, true),
        ConnectionSort::Oldest => by_time(&mut list, false),
        ConnectionSort::UploadDesc => by_traffic(&mut list, |c| c.upload),
        ConnectionSort::DownloadDesc => by_traffic(&mut list, |c| c.download),
        ConnectionSort::TotalDesc => by_traffic(&mut list, |c| c.upload + c.download),
    }
    list
}

// --- Logs ----------------------------------------------------------------------

/// `ERROR`, `WARNING` or `INFO` (ClashBar's `normalizedLogLevel`).
pub fn normalized_level(raw: &str) -> &'static str {
    let lower = raw.trim().to_lowercase();
    if lower.contains("error") || lower.contains("err") {
        "ERROR"
    } else if lower.contains("warn") {
        "WARNING"
    } else {
        "INFO"
    }
}

/// A mihomo log line's parts (ClashBar's `parseLogMessage`): its
/// `[TCP]`-style tag, the message, and a trailing `[...]` detail.
pub struct ParsedLog {
    pub protocol_tag: Option<String>,
    pub main_text: String,
    pub detail_text: Option<String>,
}

pub fn parse_log_message(raw: &str) -> ParsedLog {
    let mut message = raw.trim().to_owned();
    if message.is_empty() {
        return ParsedLog { protocol_tag: None, main_text: String::new(), detail_text: None };
    }
    // msg="..."
    if let Some(start) = message.find("msg=\"") {
        let rest = &message[start + 5..];
        if let Some(end) = rest.find('"') {
            if end > 0 {
                message = rest[..end].to_owned();
            }
        }
    }
    // A trailing `[...]` (no nested brackets) after whitespace or at the start.
    let mut detail = None;
    let trimmed_end = message.trim_end();
    if trimmed_end.ends_with(']') {
        if let Some(open) = trimmed_end.rfind('[') {
            let inner = &trimmed_end[open + 1..trimmed_end.len() - 1];
            let preceded = open == 0 || trimmed_end[..open].ends_with(char::is_whitespace);
            if preceded && !inner.contains('[') && !inner.contains(']') && !inner.is_empty() {
                let bracket = trimmed_end[open..].to_owned();
                message = message.replacen(&bracket, "", 1).trim().to_owned();
                detail = Some(bracket);
            }
        }
    }
    let mut tag = None;
    for t in ["[TCP]", "[UDP]", "[DNS]", "[HTTP]", "[HTTPS]"] {
        if let Some(i) = message.find(t) {
            if tag.as_ref().is_none_or(|(j, _): &(usize, &str)| i < *j) {
                tag = Some((i, t));
            }
        }
    }
    let protocol_tag = tag.map(|(_, t)| {
        message = message.replacen(t, "", 1).trim().to_owned();
        t.to_owned()
    });
    if message.is_empty() {
        message = raw.trim().to_owned();
    }
    ParsedLog { protocol_tag, main_text: message, detail_text: detail }
}

/// A log line of the `/logs` stream (ClashBar's `decodeLogLinePayload`).
pub fn decode_log_payload(payload: &str) -> Option<(String, String)> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: Option<String>,
        payload: Option<String>,
    }
    #[derive(Deserialize)]
    struct Lines {
        logs: Option<Vec<Line>>,
    }
    let pick = |l: &Line| {
        let message = l.payload.as_deref().unwrap_or("").trim().to_owned();
        let level = l.kind.clone().filter(|k| !k.is_empty()).unwrap_or_else(|| "info".into());
        (!message.is_empty()).then_some((level, message))
    };
    if let Ok(line) = serde_json::from_str::<Line>(payload) {
        if let Some(found) = pick(&line) {
            return Some(found);
        }
    }
    if let Ok(lines) = serde_json::from_str::<Lines>(payload) {
        if let Some(found) = lines.logs.as_ref().and_then(|l| l.first()).and_then(pick) {
            return Some(found);
        }
    }
    let text = payload.trim();
    (!text.is_empty()).then(|| ("info".to_owned(), text.to_owned()))
}

/// `yyyy-MM-dd HH:mm:ss` in local time.
pub fn date_time(at: std::time::SystemTime) -> String {
    let secs = at.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// `HH:mm:ss` of an ISO date-time, in local time.
pub fn time_only(iso: Option<&str>) -> String {
    match iso.and_then(crate::format::parse_iso8601) {
        Some(secs) => {
            let full = date_time(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs.max(0) as u64));
            full.split(' ').next_back().unwrap_or(&full).to_owned()
        }
        None => "--".into(),
    }
}

/// A log file written on a thread of its own, rotated at 10 MB with 5
/// backups (ClashBar's `AppLogStore`).
pub struct LogFile {
    tx: mpsc::Sender<LogFileOp>,
}

enum LogFileOp {
    Append(String),
    Clear,
}

impl LogFile {
    const MAX_BYTES: u64 = 10 * 1024 * 1024;
    const BACKUPS: usize = 5;

    pub fn open(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<LogFileOp>();
        std::thread::Builder::new()
            .name("mihobar-log".into())
            .spawn(move || {
                let _ = std::fs::OpenOptions::new().create(true).append(true).open(&path);
                for op in rx {
                    match op {
                        LogFileOp::Clear => {
                            let _ = std::fs::write(&path, b"");
                        }
                        LogFileOp::Append(text) => {
                            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            if size + text.len() as u64 > Self::MAX_BYTES {
                                let backup = |i: usize| PathBuf::from(format!("{}.{i}", path.display()));
                                let _ = std::fs::remove_file(backup(Self::BACKUPS));
                                for i in (1..Self::BACKUPS).rev() {
                                    let _ = std::fs::rename(backup(i), backup(i + 1));
                                }
                                if size > 0 {
                                    let _ = std::fs::rename(&path, backup(1));
                                }
                            }
                            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                                let _ = f.write_all(text.as_bytes());
                            }
                        }
                    }
                }
            })
            .ok();
        Self { tx }
    }

    pub fn append(&self, lines: &[(std::time::SystemTime, &str, &str)]) {
        let text: String = lines
            .iter()
            .map(|(at, level, message)| format!("[{}] [{}] {message}\n", date_time(*at), level.to_uppercase()))
            .collect();
        let _ = self.tx.send(LogFileOp::Append(text));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(LogFileOp::Clear);
    }
}

// --- Rules ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct RuleItem {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
}

impl RuleItem {
    fn search_text(&self) -> String {
        format!(
            "{} {} {}",
            self.payload.as_deref().unwrap_or("").trim(),
            self.kind.as_deref().unwrap_or("").trim(),
            self.proxy.as_deref().unwrap_or("").trim()
        )
    }
    pub fn policy(&self) -> String {
        self.proxy.as_deref().unwrap_or("").trim().to_owned()
    }
}

/// The rules (the first 20000) and how many the core has.
pub fn parse_rules(body: &[u8]) -> Option<(Vec<RuleItem>, usize)> {
    #[derive(Deserialize)]
    struct Rules {
        #[serde(default)]
        rules: Option<Vec<RuleItem>>,
    }
    let r: Rules = serde_json::from_slice(body).ok()?;
    let all = r.rules.unwrap_or_default();
    let total = all.len();
    Some((all.into_iter().take(RETAINED_RULES).collect(), total))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleTypeFilter {
    All,
    Domain,
    Ip,
    RuleSet,
    Other,
}

impl RuleTypeFilter {
    pub const ALL: [RuleTypeFilter; 5] = [Self::All, Self::Domain, Self::Ip, Self::RuleSet, Self::Other];
    pub fn title_key(self) -> &'static str {
        match self {
            Self::All => "ui.rules.type.all",
            Self::Domain => "ui.rules.type.domain",
            Self::Ip => "ui.rules.type.ip",
            Self::RuleSet => "ui.rules.type.ruleset",
            Self::Other => "ui.rules.type.other",
        }
    }
    pub fn matches(self, kind: Option<&str>) -> bool {
        if self == Self::All {
            return true;
        }
        let n = kind.unwrap_or("").trim().to_lowercase();
        if n.is_empty() {
            return self == Self::Other;
        }
        if n.contains("rule-set") || n.contains("ruleset") {
            return self == Self::RuleSet;
        }
        if n.contains("domain") || n.contains("geosite") {
            return self == Self::Domain;
        }
        if n.contains("ip") || n.contains("geoip") {
            return self == Self::Ip;
        }
        self == Self::Other
    }
}

/// What the rules tab shows (ClashBar's `PresentRulesOutput`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RulesView {
    pub rules: Vec<RuleItem>,
    /// Policy and its rules, the largest first (grouped mode).
    pub groups: Vec<(String, Vec<RuleItem>)>,
    pub policies: Vec<String>,
    pub type_counts: BTreeMap<&'static str, usize>,
}

pub fn present_rules(items: &[RuleItem], keyword: &str, kind: RuleTypeFilter, policy: &str, group: bool) -> RulesView {
    let mut policies: Vec<String> = items
        .iter()
        .map(RuleItem::policy)
        .filter(|p| !p.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    policies.sort_by(|a, b| crate::proxy::natural_cmp(a, b));
    let keyword = keyword.trim().to_lowercase();
    let base: Vec<&RuleItem> = items
        .iter()
        .filter(|r| policy.is_empty() || r.policy() == policy)
        .filter(|r| keyword.is_empty() || r.search_text().to_lowercase().contains(&keyword))
        .collect();
    let mut type_counts = BTreeMap::new();
    type_counts.insert(RuleTypeFilter::All.title_key(), base.len());
    for r in &base {
        if let Some(f) = RuleTypeFilter::ALL[1..].iter().find(|f| f.matches(r.kind.as_deref())) {
            *type_counts.entry(f.title_key()).or_insert(0) += 1;
        }
    }
    let rules: Vec<RuleItem> = base.into_iter().filter(|r| kind.matches(r.kind.as_deref())).cloned().collect();
    let groups = if group {
        let mut buckets: BTreeMap<String, Vec<RuleItem>> = BTreeMap::new();
        for r in &rules {
            buckets.entry(r.policy()).or_default().push(r.clone());
        }
        let mut g: Vec<(String, Vec<RuleItem>)> = buckets.into_iter().collect();
        g.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| crate::proxy::natural_cmp(&a.0, &b.0)));
        g
    } else {
        Vec::new()
    };
    RulesView { rules, groups, policies, type_counts }
}

/// The core log levels (ClashBar's `ConfigLogLevel`).
pub const LOG_LEVELS: [&str; 5] = ["silent", "error", "warning", "info", "debug"];

/// The `level` the logs stream asks for: the core's, when it is one.
pub fn stream_log_level(core_level: &str) -> Option<String> {
    let l = core_level.trim().to_lowercase();
    LOG_LEVELS.contains(&l.as_str()).then_some(l)
}

// --- Actions -------------------------------------------------------------------

use crate::api::{self, Endpoint};
use crate::model::Model;

impl Model {
    async fn run_action(&self, action: String, endpoint: Endpoint) -> bool {
        let target = self.target.get_untracked();
        match api::request_no_response(&target, &endpoint).await {
            Ok(()) => {
                self.log_app("info", self.tr_args("log.action.success", &[&action]));
                true
            }
            Err(e) => {
                self.log_app("error", self.tr_args("log.action.failed", &[&action, &e]));
                false
            }
        }
    }

    /// Reads the connections once (ClashBar's `refreshConnections`).
    pub async fn refresh_connections(&self) {
        let target = self.target.get_untracked();
        if let Ok(body) = api::request_raw(&target, &Endpoint::Connections).await {
            if let Ok(text) = std::str::from_utf8(&body) {
                if let Some((list, total)) = parse_connections(text) {
                    self.connections_count.set(total);
                    self.connections.set(list);
                }
            }
        }
    }

    pub async fn close_all_connections(&self) {
        let action = self.tr_args("log.action_name.close_all_connections", &[]);
        if self.run_action(action, Endpoint::CloseAllConnections).await {
            self.refresh_connections().await;
        }
    }

    pub async fn close_connection(&self, id: String) {
        let action = self.tr_args("log.action_name.close_connection", &[&id]);
        if self.run_action(action, Endpoint::CloseConnection(id)).await {
            self.refresh_connections().await;
        }
    }

    pub fn copy_connection_host(&self, host: &str) {
        nsh_core::prelude::set_clipboard(host.to_owned());
        self.log_app("info", self.tr_args("log.connection.copy_host", &[&host]));
    }

    pub fn copy_connection_id(&self, id: &str) {
        nsh_core::prelude::set_clipboard(id.to_owned());
        self.log_app("info", self.tr_args("log.connection.copy_id", &[&id]));
    }

    /// Reads the rules and the rule providers (ClashBar's
    /// `refreshProvidersAndRules`, its rules part).
    pub async fn refresh_providers_and_rules(&self) {
        #[derive(Deserialize)]
        struct Summary {
            providers: std::collections::HashMap<String, crate::proxy::ProviderDetail>,
        }
        let target = self.target.get_untracked();
        match api::request::<Summary>(&target, &Endpoint::RuleProviders).await {
            Ok(s) => {
                let next: BTreeMap<_, _> = s.providers.into_iter().collect();
                if self.rule_providers.get_untracked() != next {
                    self.rule_providers.set(next);
                }
            }
            Err(e) => self.log_app("error", self.tr_args("log.providers.fetch_rule_failed", &[&e])),
        }
        if let Ok(body) = api::request_raw(&target, &Endpoint::Rules).await {
            if let Some((rules, total)) = parse_rules(&body) {
                self.rules_count.set(total);
                if self.rules.get_untracked() != rules {
                    self.rules.set(rules);
                }
            }
        }
    }

    /// Updates every rule provider, one after another (ClashBar's
    /// `refreshRuleProviders`).
    pub async fn refresh_rule_providers(&self) {
        if self.rule_providers_refreshing.get_untracked() {
            return;
        }
        self.rule_providers_refreshing.set(true);
        #[derive(Deserialize)]
        struct Summary {
            providers: std::collections::HashMap<String, Value>,
        }
        let target = self.target.get_untracked();
        match api::request::<Summary>(&target, &Endpoint::RuleProviders).await {
            Ok(s) => {
                let mut names: Vec<String> = s.providers.into_keys().collect();
                names.sort();
                for name in names {
                    if let Err(e) = api::request_no_response(&target, &Endpoint::UpdateRuleProvider(name.clone())).await {
                        self.log_app("error", self.tr_args("log.providers.rule_update_failed", &[&name, &e]));
                    }
                }
            }
            Err(e) => self.log_app("error", self.tr_args("log.providers.fetch_rule_failed", &[&e])),
        }
        self.refresh_providers_and_rules().await;
        self.rule_providers_refreshing.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connections_and_rules() {
        let json = r#"{"connections":[{"id":"a","upload":5,"download":7,"start":"2026-11-24T10:00:00Z",
            "rule":"RuleSet","rulePayload":"GitHub","chains":["node","GitHub"],
            "metadata":{"network":"tcp","host":"github.com","destinationIP":"1.2.3.4"}}]}"#;
        let (list, total) = parse_connections(json).unwrap();
        assert_eq!(total, 1);
        assert_eq!(list[0].chain_parts(), vec!["GitHub", "node"]);
        assert_eq!(list[0].resolved_host().as_deref(), Some("github.com"));
        assert_eq!(parse_rule(Some("RuleSet(GitHub)")), Some(("RuleSet".into(), Some("GitHub".into()))));
        assert_eq!(parse_rule(Some("DOMAIN,a.com")), Some(("DOMAIN".into(), Some("a.com".into()))));
        assert_eq!(rule_type_text(Some("Match"), None), "--");
        assert!(TransportFilter::Other.matches(Some("quic")));
        assert!(RuleTypeFilter::RuleSet.matches(Some("RuleSet")));
        assert!(RuleTypeFilter::Domain.matches(Some("DomainSuffix")));
    }

    #[test]
    fn parses_log_lines() {
        let p = parse_log_message(r#"time="x" level=info msg="[TCP] a --> b:443 match Match using X [DIRECT]""#);
        assert_eq!(p.protocol_tag.as_deref(), Some("[TCP]"));
        assert_eq!(p.detail_text.as_deref(), Some("[DIRECT]"));
        assert_eq!(p.main_text, "a --> b:443 match Match using X");
        assert_eq!(normalized_level("warning"), "WARNING");
        assert_eq!(decode_log_payload(r#"{"type":"warning","payload":"x"}"#), Some(("warning".into(), "x".into())));
    }
}
