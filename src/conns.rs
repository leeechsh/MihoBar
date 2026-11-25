//! The advanced window's connections (zashboard's connections page): every
//! connection the core reports, with its speeds from one snapshot to the
//! next, the last 500 closed ones, and the table's columns, search, hiding,
//! sorting and grouping.

use std::collections::HashMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Closed connections kept (zashboard: 500).
pub const CLOSED_KEPT: usize = 500;

/// A connection as the table shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Conn {
    pub id: String,
    pub source_ip: String,
    pub source_port: String,
    /// `host:port` (zashboard's `host`).
    pub host: String,
    pub sniff_host: String,
    /// `inbound type | network` (zashboard's `networkType`).
    pub kind: String,
    pub network: String,
    pub process: String,
    /// `rule: payload`.
    pub rule: String,
    /// As the core lists them: the node first, the group last.
    pub chains: Vec<String>,
    pub download: u64,
    pub upload: u64,
    pub dl_speed: u64,
    pub ul_speed: u64,
    /// Unix milliseconds.
    pub start: i64,
    pub destination: String,
    pub destination_type: String,
    pub remote_address: String,
    pub inbound_user: String,
    pub closed: bool,
    /// The core's record, for the details.
    pub raw: Raw,
}

/// The core's record of a connection, compared by identity (a snapshot's
/// records are new each time, and comparing them whole every second would
/// cost more than it saves).
#[derive(Clone, Debug)]
pub struct Raw(pub Rc<Value>);

impl PartialEq for Raw {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

fn field(meta: Option<&Value>, key: &str) -> String {
    match meta.and_then(|m| m.get(key)) {
        Some(Value::String(s)) => s.trim().to_owned(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

impl Conn {
    pub fn from_value(v: &Value) -> Option<Self> {
        let id = v.get("id")?.as_str()?.to_owned();
        let meta = v.get("metadata");
        let f = |k: &str| field(meta, k);
        let hostname = [f("host"), f("sniffHost"), f("destinationIP")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
        let port = f("destinationPort");
        let host = if hostname.contains(':') { format!("[{hostname}]:{port}") } else { format!("{hostname}:{port}") };
        let mut network = f("network");
        if (port == "443" || !f("sniffHost").is_empty()) && network == "udp" {
            network = "quic".into();
        }
        let rule = v.get("rule").and_then(Value::as_str).unwrap_or_default();
        let payload = v.get("rulePayload").and_then(Value::as_str).unwrap_or_default();
        let process = [f("process"), f("processPath").rsplit(['/', '\\']).next().unwrap_or_default().to_owned()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| "-".into());
        let destination_ip = f("destinationIP");
        let destination = if destination_ip.is_empty() { f("host") } else { destination_ip.clone() };
        let destination_type = if destination_ip.contains(':') {
            "IPv6"
        } else if !destination_ip.is_empty() {
            "IPv4"
        } else {
            ""
        };
        let inbound_user = [f("inboundUser"), f("inboundName"), f("inboundPort")].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| "-".into());
        Some(Self {
            id,
            source_ip: f("sourceIP"),
            source_port: f("sourcePort"),
            host,
            sniff_host: f("sniffHost"),
            kind: format!("{} | {network}", f("type")),
            network,
            process,
            rule: if payload.is_empty() { rule.to_owned() } else { format!("{rule}: {payload}") },
            chains: v.get("chains").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(),
            download: v.get("download").and_then(Value::as_u64).unwrap_or(0),
            upload: v.get("upload").and_then(Value::as_u64).unwrap_or(0),
            dl_speed: 0,
            ul_speed: 0,
            start: v.get("start").and_then(Value::as_str).and_then(crate::format::parse_iso8601_ms).unwrap_or(0),
            destination,
            destination_type: destination_type.into(),
            remote_address: f("remoteDestination"),
            inbound_user,
            closed: false,
            raw: Raw(Rc::new(v.clone())),
        })
    }

    /// The chain as shown: the group first, the node last.
    pub fn chain_text(&self) -> String {
        self.chains.iter().rev().cloned().collect::<Vec<_>>().join(" → ")
    }

    pub fn outbound(&self) -> &str {
        self.chains.first().map_or("-", String::as_str)
    }
}

/// The active and closed connections, from snapshot to snapshot.
#[derive(Default)]
pub struct Tracker {
    previous: HashMap<String, (u64, u64)>,
}

impl Tracker {
    /// The snapshot's connections with their speeds, and those it no longer
    /// has (closed since the last one).
    pub fn apply(&mut self, snapshot: &Value, before: &[Conn]) -> (Vec<Conn>, Vec<Conn>) {
        let list = snapshot.get("connections").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        let mut next = HashMap::with_capacity(list.len());
        let active: Vec<Conn> = list
            .iter()
            .filter_map(Conn::from_value)
            .map(|mut c| {
                if let Some((up, down)) = self.previous.get(&c.id) {
                    c.ul_speed = c.upload.saturating_sub(*up);
                    c.dl_speed = c.download.saturating_sub(*down);
                }
                next.insert(c.id.clone(), (c.upload, c.download));
                c
            })
            .collect();
        let closed = before
            .iter()
            .filter(|c| !next.contains_key(&c.id))
            .map(|c| Conn { closed: true, dl_speed: 0, ul_speed: 0, ..c.clone() })
            .collect();
        self.previous = next;
        (active, closed)
    }
}

// --- Columns ------------------------------------------------------------------------

/// The table's columns (zashboard's `CONNECTIONS_TABLE_ACCESSOR_KEY`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Column {
    Close,
    SourceIp,
    SourcePort,
    Host,
    SniffHost,
    Type,
    Process,
    Rule,
    Chains,
    Outbound,
    ConnectTime,
    DlSpeed,
    UlSpeed,
    Download,
    Upload,
    Destination,
    DestinationType,
    RemoteAddress,
    InboundUser,
}

impl Column {
    pub const ALL: [Column; 19] = [
        Self::Close,
        Self::SourceIp,
        Self::SourcePort,
        Self::Host,
        Self::SniffHost,
        Self::Type,
        Self::Process,
        Self::Rule,
        Self::Chains,
        Self::Outbound,
        Self::ConnectTime,
        Self::DlSpeed,
        Self::UlSpeed,
        Self::Download,
        Self::Upload,
        Self::Destination,
        Self::DestinationType,
        Self::RemoteAddress,
        Self::InboundUser,
    ];

    /// What the table shows by default (the order of leeechsh's setup).
    pub const DEFAULT: [Column; 11] = [
        Self::SourceIp,
        Self::Host,
        Self::Type,
        Self::Rule,
        Self::Chains,
        Self::DlSpeed,
        Self::UlSpeed,
        Self::Download,
        Self::Upload,
        Self::ConnectTime,
        Self::Close,
    ];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Close => "z.col.close",
            Self::SourceIp => "z.col.source_ip",
            Self::SourcePort => "z.col.source_port",
            Self::Host => "z.col.host",
            Self::SniffHost => "z.col.sniff_host",
            Self::Type => "z.col.type",
            Self::Process => "z.col.process",
            Self::Rule => "z.col.rule",
            Self::Chains => "z.col.chains",
            Self::Outbound => "z.col.outbound",
            Self::ConnectTime => "z.col.connect_time",
            Self::DlSpeed => "z.col.dl_speed",
            Self::UlSpeed => "z.col.ul_speed",
            Self::Download => "z.col.download",
            Self::Upload => "z.col.upload",
            Self::Destination => "z.col.destination",
            Self::DestinationType => "z.col.destination_type",
            Self::RemoteAddress => "z.col.remote_address",
            Self::InboundUser => "z.col.inbound_user",
        }
    }

    /// Its natural width in the table (zashboard's table is as wide as
    /// its columns and scrolls sideways).
    pub fn width(self) -> f32 {
        match self {
            Self::Close => 56.0,
            Self::SourceIp => 130.0,
            Self::SourcePort | Self::DestinationType => 100.0,
            Self::Host => 260.0,
            Self::SniffHost => 200.0,
            Self::Type => 120.0,
            Self::Process => 140.0,
            Self::Rule => 220.0,
            Self::Chains => 300.0,
            Self::Outbound => 160.0,
            Self::ConnectTime => 120.0,
            Self::DlSpeed | Self::UlSpeed => 96.0,
            Self::Download | Self::Upload => 90.0,
            Self::Destination => 150.0,
            Self::RemoteAddress => 160.0,
            Self::InboundUser => 140.0,
        }
    }

    /// Its share of the width left when the window is wider than the table.
    pub fn grow(self) -> f32 {
        match self {
            Self::Host => 1.0,
            Self::Chains => 1.4,
            Self::Rule => 0.6,
            _ => 0.0,
        }
    }

    pub fn sortable(self) -> bool {
        self != Self::Close
    }

    /// Rows can be grouped by it (zashboard's groupable keys).
    pub fn groupable(self) -> bool {
        matches!(
            self,
            Self::SourceIp
                | Self::Host
                | Self::SniffHost
                | Self::Type
                | Self::Process
                | Self::Rule
                | Self::Chains
                | Self::Outbound
                | Self::Destination
                | Self::DestinationType
                | Self::InboundUser
        )
    }

    /// Numbers sort largest first on the first press.
    pub fn numeric(self) -> bool {
        matches!(self, Self::DlSpeed | Self::UlSpeed | Self::Download | Self::Upload | Self::ConnectTime)
    }

    /// The cell's text (also what search and grouping read).
    pub fn text(self, c: &Conn, now_ms: i64, zh: bool) -> String {
        let bytes = crate::format::pretty_bytes;
        match self {
            Self::Close => String::new(),
            Self::SourceIp => c.source_ip.clone(),
            Self::SourcePort => c.source_port.clone(),
            Self::Host => c.host.clone(),
            Self::SniffHost => c.sniff_host.clone(),
            Self::Type => c.kind.clone(),
            Self::Process => c.process.clone(),
            Self::Rule => c.rule.clone(),
            Self::Chains => c.chain_text(),
            Self::Outbound => c.outbound().to_owned(),
            Self::ConnectTime => crate::format::from_now(c.start, now_ms, zh),
            Self::DlSpeed => format!("{}/s", bytes(c.dl_speed)),
            Self::UlSpeed => format!("{}/s", bytes(c.ul_speed)),
            Self::Download => bytes(c.download),
            Self::Upload => bytes(c.upload),
            Self::Destination => c.destination.clone(),
            Self::DestinationType => c.destination_type.clone(),
            Self::RemoteAddress => c.remote_address.clone(),
            Self::InboundUser => c.inbound_user.clone(),
        }
    }

    fn number(self, c: &Conn) -> Option<i64> {
        match self {
            Self::DlSpeed => Some(c.dl_speed as i64),
            Self::UlSpeed => Some(c.ul_speed as i64),
            Self::Download => Some(c.download as i64),
            Self::Upload => Some(c.upload as i64),
            Self::ConnectTime => Some(c.start),
            _ => None,
        }
    }
}

// --- The window's settings ----------------------------------------------------------

/// What the advanced window keeps between openings (zashboard keeps them in
/// the browser): its own file, `state/advanced.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    pub columns: Vec<Column>,
    pub hide_regex: String,
    pub hide_enabled: bool,
    pub search_hidden_columns: bool,
    pub sort: Option<(Column, bool)>,
    pub group_by: Option<Column>,
    pub log_level: String,
    pub log_search_hidden: bool,
    /// Logs kept (zashboard's `logRetentionLimit`).
    pub log_retention: usize,
    pub log_hide_regex: String,
    pub log_hide_enabled: bool,
    /// Rules: the group's selected node and its latency in the chain, and
    /// closing a rule's connections as it is disabled.
    pub rule_show_now: bool,
    pub rule_show_latency: bool,
    pub rule_disconnect_on_disable: bool,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            columns: Column::DEFAULT.to_vec(),
            hide_regex: "direct|dns-out".into(),
            hide_enabled: false,
            search_hidden_columns: false,
            sort: None,
            group_by: None,
            log_level: "info".into(),
            log_search_hidden: false,
            log_retention: 1000,
            log_hide_regex: String::new(),
            log_hide_enabled: false,
            rule_show_now: true,
            rule_show_latency: true,
            rule_disconnect_on_disable: false,
        }
    }
}

impl AdvancedSettings {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) {
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

// --- Filtering, sorting, grouping -------------------------------------------------

/// A search as zashboard reads it: a case-insensitive regex, else the text.
pub fn search_regex(text: &str) -> Option<regex::Regex> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    regex::RegexBuilder::new(text)
        .case_insensitive(true)
        .build()
        .or_else(|_| regex::RegexBuilder::new(&regex::escape(text)).case_insensitive(true).build())
        .ok()
}

pub struct View<'a> {
    pub search: &'a str,
    pub search_hidden: bool,
    pub hide: Option<&'a str>,
    pub source_ip: Option<&'a str>,
    pub columns: &'a [Column],
    pub sort: Option<(Column, bool)>,
    pub now_ms: i64,
    pub zh: bool,
}

/// The rows the table shows (zashboard's `renderConnections`): filtered by
/// source IP, the hiding regex and the search, then sorted (by host unless
/// a column is chosen).
pub fn render(items: &[Conn], view: &View) -> Vec<Conn> {
    let search = search_regex(view.search);
    let hide = view.hide.and_then(search_regex);
    let search_columns: &[Column] = if view.search_hidden { &Column::ALL } else { view.columns };
    let mut out: Vec<Conn> = items
        .iter()
        .filter(|c| view.source_ip.is_none_or(|ip| c.source_ip == ip))
        .filter(|c| {
            let texts = |cols: &[Column]| cols.iter().map(|col| col.text(c, view.now_ms, view.zh)).collect::<Vec<_>>();
            if let Some(hide) = &hide {
                if texts(&Column::ALL).iter().any(|t| hide.is_match(t)) {
                    return false;
                }
            }
            match &search {
                Some(re) => texts(search_columns).iter().any(|t| re.is_match(t)),
                None => true,
            }
        })
        .cloned()
        .collect();
    let (column, desc) = view.sort.unwrap_or((Column::Host, false));
    out.sort_by(|a, b| {
        let order = match (column.number(a), column.number(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            _ => column.text(a, view.now_ms, view.zh).to_lowercase().cmp(&column.text(b, view.now_ms, view.zh).to_lowercase()),
        };
        let order = if desc { order.reverse() } else { order };
        order.then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// A row of a grouped table: a group's header or a connection.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Group { value: String, count: usize, expanded: bool },
    Conn(Conn),
}

/// The rows grouped by `column` (groups in order of first appearance), each
/// group's connections listed only while it is expanded.
pub fn grouped(rows: Vec<Conn>, column: Column, expanded: &std::collections::HashSet<String>, now_ms: i64, zh: bool) -> Vec<Row> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Conn>> = HashMap::new();
    for c in rows {
        let key = column.text(&c, now_ms, zh);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(c);
    }
    let mut out = Vec::new();
    for key in order {
        let members = groups.remove(&key).unwrap_or_default();
        let open = expanded.contains(&key);
        out.push(Row::Group { value: key, count: members.len(), expanded: open });
        if open {
            out.extend(members.into_iter().map(Row::Conn));
        }
    }
    out
}

/// A core log line as the logs page lists it (zashboard's `LogWithSeq`).
#[derive(Clone, Debug, PartialEq)]
pub struct ZLog {
    pub seq: u64,
    pub level: String,
    /// `HH:mm:ss`.
    pub time: String,
    pub payload: String,
}

impl ZLog {
    /// Its type: the payload's first word (`[TCP]`, `[DNS]`...).
    pub fn kind(&self) -> &str {
        self.payload.split(' ').next().unwrap_or("")
    }
}

/// The logs the page lists: by level or type, without those the hiding
/// regex matches, and those the search matches.
pub fn filter_logs(logs: &[ZLog], type_filter: &str, search: &str, hide: Option<&str>) -> Vec<ZLog> {
    let search = search_regex(search);
    let hide = hide.and_then(search_regex);
    logs.iter()
        .filter(|l| type_filter.is_empty() || l.level == type_filter || l.kind() == type_filter)
        .filter(|l| {
            let texts = [l.level.as_str(), l.time.as_str(), l.payload.as_str()];
            hide.as_ref().is_none_or(|h| !texts.iter().any(|t| h.is_match(t))) && search.as_ref().is_none_or(|s| texts.iter().any(|t| s.is_match(t)))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn conn(id: &str, host: &str, down: u64, rule: &str) -> Value {
        json!({
            "id": id, "upload": 10, "download": down, "start": "2026-11-25T12:00:00.000Z",
            "chains": ["DIRECT", "国内服务"], "rule": "RuleSet", "rulePayload": rule,
            "metadata": {"network": "tcp", "type": "Redir", "sourceIP": "192.168.99.170", "sourcePort": "50000",
                         "host": host, "destinationIP": "1.2.3.4", "destinationPort": "443", "inboundName": "DEFAULT-REDIR"}
        })
    }

    #[test]
    fn reads_a_connection_like_zashboard() {
        let c = Conn::from_value(&conn("a", "browser.events.data.msn.cn", 99, "cn")).unwrap();
        assert_eq!(c.host, "browser.events.data.msn.cn:443");
        assert_eq!(c.kind, "Redir | tcp");
        assert_eq!(c.rule, "RuleSet: cn");
        assert_eq!(c.chain_text(), "国内服务 → DIRECT");
        assert_eq!(c.inbound_user, "DEFAULT-REDIR");
        assert_eq!(c.destination_type, "IPv4");
    }

    #[test]
    fn speeds_and_closed() {
        let mut t = Tracker::default();
        let (first, _) = t.apply(&json!({"connections": [conn("a", "x.com", 100, "cn"), conn("b", "y.com", 5, "cn")]}), &[]);
        assert_eq!(first[0].dl_speed, 0);
        let (second, closed) = t.apply(&json!({"connections": [conn("a", "x.com", 600, "cn")]}), &first);
        assert_eq!(second[0].dl_speed, 500);
        assert_eq!(closed.len(), 1);
        assert!(closed[0].closed);
        assert_eq!(closed[0].id, "b");
    }

    #[test]
    fn search_hide_sort_group() {
        let items: Vec<Conn> = [("a", "b.com", 1), ("b", "a.com", 3), ("c", "c.com", 2)]
            .iter()
            .map(|(id, h, d)| Conn::from_value(&conn(id, h, *d, "cn")).unwrap())
            .collect();
        let base = View { search: "", search_hidden: false, hide: None, source_ip: None, columns: &Column::DEFAULT, sort: None, now_ms: 0, zh: false };
        let hosts = |v: &View| render(&items, v).iter().map(|c| c.host.clone()).collect::<Vec<_>>();
        assert_eq!(hosts(&base), ["a.com:443", "b.com:443", "c.com:443"]);
        assert_eq!(hosts(&View { search: "^b\\.", ..base }), ["b.com:443"]);
        assert_eq!(hosts(&View { search: "C.COM", ..base }), ["c.com:443"]);
        assert_eq!(hosts(&View { sort: Some((Column::Download, true)), ..base }), ["a.com:443", "c.com:443", "b.com:443"]);
        assert!(render(&items, &View { hide: Some("direct"), ..base }).is_empty());
        let rows = grouped(render(&items, &base), Column::Rule, &Default::default(), 0, false);
        assert_eq!(rows, vec![Row::Group { value: "RuleSet: cn".into(), count: 3, expanded: false }]);
    }
}
