//! Data usage (the usage dashboard of zashboard, leeechsh's fork): how
//! much each device, host, outbound, process and inbound user moved.
//!
//! zashboard counts only while its page is open and keeps the counts in
//! the browser. Mihobar's menu bar process counts whenever its core runs:
//! it reads the core's connections every few seconds, adds each
//! connection's traffic since the last reading to a per-minute bucket, and
//! appends the buckets to one file a day under `usage/`. Files older than
//! two days are folded into hourly buckets; files past the retention
//! (a month by default) are deleted. The advanced window reads the files.
//!
//! A line: `minute\tsource ip\thost\toutbound\tprocess\tinbound user\tup\tdown`
//! (the minute as Unix seconds; tabs and line breaks in names become
//! spaces).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How often the buffered buckets are written (zashboard: 30 s).
const FLUSH_SECS: i64 = 30;
/// Minute buckets are kept this long before they are folded into hours.
const MINUTE_DETAIL_SECS: i64 = 48 * 3600;
/// Retention by default: a month.
pub const DEFAULT_RETENTION_SECS: i64 = 30 * 86400;

/// What a record is counted by (zashboard's `DataUsageDimension`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dimension {
    SourceIp,
    InboundUser,
    Host,
    Outbound,
    Process,
}

impl Dimension {
    pub const ALL: [Dimension; 5] = [Self::SourceIp, Self::InboundUser, Self::Host, Self::Outbound, Self::Process];
}

/// Traffic of one key in one bucket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The bucket's start, Unix seconds.
    pub at: i64,
    pub source_ip: String,
    pub host: String,
    pub outbound: String,
    pub process: String,
    pub inbound_user: String,
    pub upload: u64,
    pub download: u64,
}

impl Record {
    pub fn get(&self, d: Dimension) -> &str {
        match d {
            Dimension::SourceIp => &self.source_ip,
            Dimension::InboundUser => &self.inbound_user,
            Dimension::Host => &self.host,
            Dimension::Outbound => &self.outbound,
            Dimension::Process => &self.process,
        }
    }

    fn line(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            self.at,
            clean(&self.source_ip),
            clean(&self.host),
            clean(&self.outbound),
            clean(&self.process),
            clean(&self.inbound_user),
            self.upload,
            self.download
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let mut f = line.split('\t');
        let r = Self {
            at: f.next()?.parse().ok()?,
            source_ip: f.next()?.to_owned(),
            host: f.next()?.to_owned(),
            outbound: f.next()?.to_owned(),
            process: f.next()?.to_owned(),
            inbound_user: f.next()?.to_owned(),
            upload: f.next()?.parse().ok()?,
            download: f.next()?.trim_end().parse().ok()?,
        };
        f.next().is_none().then_some(r)
    }

    fn key(&self) -> Key {
        (self.at, self.source_ip.clone(), self.host.clone(), self.outbound.clone(), self.process.clone(), self.inbound_user.clone())
    }
}

type Key = (i64, String, String, String, String, String);

/// What the advanced window sets and the recorder follows (read again at
/// every flush): whether to count, how long to keep, and when it was last
/// cleared (counts from before are dropped).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageConfig {
    pub tracking: bool,
    /// Seconds; -1 keeps everything.
    pub retention_secs: i64,
    pub cleared_at: i64,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self { tracking: true, retention_secs: DEFAULT_RETENTION_SECS, cleared_at: 0 }
    }
}

impl UsageConfig {
    pub fn load(dir: &Path) -> Self {
        std::fs::read(dir.join("config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        let _ = std::fs::create_dir_all(dir);
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = dir.join("config.json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(tmp, dir.join("config.json"));
            }
        }
    }
}

// --- Days -------------------------------------------------------------------

fn local_tm(secs: i64) -> libc::tm {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: valid pointers; localtime_r fills tm.
    unsafe { libc::localtime_r(&t, &mut tm) };
    tm
}

/// The local day `secs` falls in, as its file stem (`YYYY-MM-DD`).
fn day_name(secs: i64) -> String {
    let tm = local_tm(secs);
    format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

/// The start of local day `name`, and of the next one, Unix seconds.
fn day_bounds(name: &str) -> Option<(i64, i64)> {
    let mut p = name.split('-').map(|x| x.parse::<i32>());
    let (y, m, d) = (p.next()?.ok()?, p.next()?.ok()?, p.next()?.ok()?);
    let at = |day: i32| {
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = y - 1900;
        tm.tm_mon = m - 1;
        tm.tm_mday = day;
        tm.tm_isdst = -1;
        // SAFETY: a filled-in tm; mktime normalizes an overflowing day.
        unsafe { libc::mktime(&mut tm) as i64 }
    };
    Some((at(d), at(d + 1)))
}

/// The usage files: (day, hourly, path).
fn files(dir: &Path) -> Vec<(String, bool, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".tsv")?;
            let (day, hourly) = match stem.strip_suffix(".h") {
                Some(day) => (day.to_owned(), true),
                None => (stem.to_owned(), false),
            };
            day_bounds(&day)?;
            Some((day, hourly, e.path()))
        })
        .collect();
    out.sort();
    out
}

fn read_records(path: &Path) -> Vec<Record> {
    std::fs::read_to_string(path).map(|t| t.lines().filter_map(Record::parse).collect()).unwrap_or_default()
}

fn write_records(path: &Path, records: &[Record]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    for r in records {
        f.write_all(r.line().as_bytes())?;
    }
    f.flush()?;
    std::fs::rename(tmp, path)
}

/// Adds records with the same bucket and key together, the buckets made
/// `bucket` seconds long.
fn fold(records: impl IntoIterator<Item = Record>, bucket: i64) -> Vec<Record> {
    let mut map: HashMap<Key, Record> = HashMap::new();
    for mut r in records {
        r.at = r.at.div_euclid(bucket) * bucket;
        match map.get_mut(&r.key()) {
            Some(e) => {
                e.upload += r.upload;
                e.download += r.download;
            }
            None => {
                map.insert(r.key(), r);
            }
        }
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| a.at.cmp(&b.at));
    out
}

/// Folds minute files older than two days into hours, and deletes files
/// past the retention.
pub fn maintain(dir: &Path, now: i64, retention_secs: i64) {
    for (day, hourly, path) in files(dir) {
        let Some((_, end)) = day_bounds(&day) else { continue };
        if retention_secs >= 0 && end <= now - retention_secs {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        if !hourly && end <= now - MINUTE_DETAIL_SECS {
            let folded = fold(read_records(&path), 3600);
            let target = dir.join(format!("{day}.h.tsv"));
            let mut all = read_records(&target);
            all.extend(folded);
            if write_records(&target, &fold(all, 3600)).is_ok() {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// The records from `start` to `end` (Unix seconds).
pub fn load(dir: &Path, start: i64, end: i64) -> Vec<Record> {
    let mut out = Vec::new();
    for (day, _, path) in files(dir) {
        let Some((day_start, day_end)) = day_bounds(&day) else { continue };
        if day_end <= start || day_start > end {
            continue;
        }
        out.extend(read_records(&path).into_iter().filter(|r| r.at >= start - 3599 && r.at <= end));
    }
    out
}

/// Deletes every record.
pub fn clear(dir: &Path) {
    for (_, _, path) in files(dir) {
        let _ = std::fs::remove_file(path);
    }
}

// --- Recording (the menu bar process) -----------------------------------------

/// Counts the core's traffic into per-minute buckets.
pub struct Recorder {
    dir: PathBuf,
    config: UsageConfig,
    previous: HashMap<String, (u64, u64)>,
    initialized: bool,
    totals: (u64, u64),
    buffer: HashMap<Key, Record>,
    last_flush: i64,
    last_maintenance: i64,
}

impl Recorder {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        let config = UsageConfig::load(&dir);
        Self {
            dir,
            config,
            previous: HashMap::new(),
            initialized: false,
            totals: (0, 0),
            buffer: HashMap::new(),
            last_flush: 0,
            last_maintenance: 0,
        }
    }

    pub fn tracking(&self) -> bool {
        self.config.tracking
    }

    /// Starts again from the next reading (the stream closed: the core
    /// stopped or restarted).
    pub fn restart(&mut self) {
        self.previous.clear();
        self.initialized = false;
        self.totals = (0, 0);
    }

    /// A `/connections` snapshot: what each connection moved since the
    /// last one goes into this minute's buckets.
    pub fn observe(&mut self, snapshot: &Value, now: i64) {
        if now - self.last_flush >= FLUSH_SECS {
            self.flush(now);
        }
        if !self.config.tracking {
            self.restart();
            return;
        }
        let totals = (
            snapshot.get("uploadTotal").and_then(Value::as_u64).unwrap_or(0),
            snapshot.get("downloadTotal").and_then(Value::as_u64).unwrap_or(0),
        );
        if totals.0 < self.totals.0 || totals.1 < self.totals.1 {
            self.restart();
        }
        self.totals = totals;
        let connections = snapshot.get("connections").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        let read = |c: &Value| -> Option<(String, u64, u64)> {
            Some((
                c.get("id")?.as_str()?.to_owned(),
                c.get("upload").and_then(Value::as_u64).unwrap_or(0),
                c.get("download").and_then(Value::as_u64).unwrap_or(0),
            ))
        };
        if !self.initialized {
            self.initialized = true;
            self.previous = connections.iter().filter_map(read).map(|(id, u, d)| (id, (u, d))).collect();
            return;
        }
        let minute = now.div_euclid(60) * 60;
        let mut next = HashMap::with_capacity(connections.len());
        for c in connections {
            let Some((id, up, down)) = read(c) else { continue };
            let (old_up, old_down) = self.previous.get(&id).copied().unwrap_or((0, 0));
            next.insert(id, (up, down));
            let (du, dd) = (up.saturating_sub(old_up), down.saturating_sub(old_down));
            if du == 0 && dd == 0 {
                continue;
            }
            let record = describe(c, minute, du, dd);
            match self.buffer.get_mut(&record.key()) {
                Some(e) => {
                    e.upload += du;
                    e.download += dd;
                }
                None => {
                    self.buffer.insert(record.key(), record);
                }
            }
        }
        self.previous = next;
    }

    /// Writes the buffered buckets (and, hourly, folds and deletes old
    /// files); follows the window's settings.
    pub fn flush(&mut self, now: i64) {
        self.last_flush = now;
        let config = UsageConfig::load(&self.dir);
        if config.cleared_at > self.config.cleared_at {
            self.buffer.retain(|_, r| r.at >= config.cleared_at);
        }
        self.config = config;
        if !self.buffer.is_empty() {
            let mut by_day: HashMap<String, Vec<Record>> = HashMap::new();
            for (_, r) in self.buffer.drain() {
                by_day.entry(day_name(r.at)).or_default().push(r);
            }
            for (day, records) in by_day {
                let path = self.dir.join(format!("{day}.tsv"));
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let text: String = records.iter().map(Record::line).collect();
                    let _ = f.write_all(text.as_bytes());
                }
            }
        }
        if now - self.last_maintenance >= 3600 {
            self.last_maintenance = now;
            maintain(&self.dir, now, self.config.retention_secs);
        }
    }
}

/// A connection's record (zashboard's fields: the source IP, the host
/// without its port, the final outbound, the process, the inbound user).
fn describe(c: &Value, at: i64, upload: u64, download: u64) -> Record {
    let meta = c.get("metadata");
    let field = |k: &str| meta.and_then(|m| m.get(k)).and_then(|v| match v {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty() && s != "0"),
        Value::Number(n) => Some(n.to_string()).filter(|s| s != "0"),
        _ => None,
    });
    let process = field("process").or_else(|| field("processPath").map(|p| p.rsplit(['/', '\\']).next().unwrap_or(&p).to_owned()));
    Record {
        at,
        source_ip: field("sourceIP").unwrap_or_else(|| "Inner".into()),
        host: field("host").or_else(|| field("sniffHost")).or_else(|| field("destinationIP")).unwrap_or_else(|| "Unknown".into()),
        outbound: c
            .get("chains")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| "DIRECT".into()),
        process: process.unwrap_or_else(|| "Unknown".into()),
        inbound_user: field("inboundUser").or_else(|| field("inboundName")).or_else(|| field("inboundPort")).unwrap_or_else(|| "Unknown".into()),
        upload,
        download,
    }
}

// --- Reading (the advanced window) ----------------------------------------------

/// Traffic of one label (zashboard's `DataUsageAggregate`).
#[derive(Clone, Debug, PartialEq)]
pub struct Aggregate {
    pub label: String,
    pub upload: u64,
    pub download: u64,
    pub count: usize,
}

impl Aggregate {
    pub fn total(&self) -> u64 {
        self.upload + self.download
    }
}

/// The records' traffic by `dimension`, largest first.
pub fn aggregate<'a>(records: impl IntoIterator<Item = &'a Record>, dimension: Dimension) -> Vec<Aggregate> {
    let mut map: HashMap<&str, Aggregate> = HashMap::new();
    for r in records {
        let label = r.get(dimension);
        let row = map.entry(label).or_insert_with(|| Aggregate { label: label.to_owned(), upload: 0, download: 0, count: 0 });
        row.upload += r.upload;
        row.download += r.download;
        row.count += 1;
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| b.total().cmp(&a.total()).then_with(|| a.label.cmp(&b.label)));
    out
}

/// The bucket a chart of `range_secs` uses (zashboard: a minute up to an
/// hour, 5 minutes up to a day, an hour up to a week, else a day).
pub fn chart_bucket(range_secs: i64) -> i64 {
    match range_secs {
        r if r <= 3600 => 60,
        r if r <= 86400 => 300,
        r if r <= 7 * 86400 => 3600,
        _ => 86400,
    }
}

/// Upload and download per bucket from `start` to `end`.
pub fn series(records: &[Record], start: i64, end: i64, bucket: i64) -> Vec<(i64, u64, u64)> {
    let first = start.div_euclid(bucket) * bucket;
    let mut out: Vec<(i64, u64, u64)> = (0..)
        .map(|i| first + i * bucket)
        .take_while(|t| *t <= end)
        .map(|t| (t, 0, 0))
        .collect();
    for r in records {
        let i = (r.at.div_euclid(bucket) * bucket - first) / bucket;
        if let Some(slot) = usize::try_from(i).ok().and_then(|i| out.get_mut(i)) {
            slot.1 += r.upload;
            slot.2 += r.download;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mihobar-usage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn snapshot(conns: &[(&str, u64, u64, &str)]) -> Value {
        json!({
            "uploadTotal": 1000,
            "downloadTotal": 1000,
            "connections": conns.iter().map(|(id, up, down, host)| json!({
                "id": id, "upload": up, "download": down,
                "chains": ["Node-A", "Proxy"],
                "metadata": {"sourceIP": "192.168.1.2", "host": host, "process": "curl", "inboundName": "DEFAULT-MIXED"}
            })).collect::<Vec<_>>()
        })
    }

    #[test]
    fn records_deltas_by_minute() {
        let dir = tmp_dir("deltas");
        let mut r = Recorder::new(dir.clone());
        let t0 = 1_800_000_000 - 1_800_000_000 % 60;
        r.observe(&snapshot(&[("a", 100, 1000, "example.com")]), t0 + 1);
        r.observe(&snapshot(&[("a", 150, 1600, "example.com"), ("b", 10, 20, "github.com")]), t0 + 6);
        r.observe(&snapshot(&[("a", 150, 1700, "example.com")]), t0 + 11);
        r.flush(t0 + 12);
        let records = load(&dir, t0 - 60, t0 + 60);
        let rows = aggregate(&records, Dimension::Host);
        let ex = rows.iter().find(|a| a.label == "example.com").unwrap();
        assert_eq!((ex.upload, ex.download), (50, 700));
        let gh = rows.iter().find(|a| a.label == "github.com").unwrap();
        assert_eq!((gh.upload, gh.download), (10, 20));
        assert_eq!(aggregate(&records, Dimension::Outbound)[0].label, "Node-A");
        assert_eq!(aggregate(&records, Dimension::InboundUser)[0].label, "DEFAULT-MIXED");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn folds_old_days_and_drops_expired() {
        let dir = tmp_dir("fold");
        let now = 1_800_000_000;
        let old_day = now - 3 * 86400;
        let rec = |at: i64| Record {
            at,
            source_ip: "s".into(),
            host: "h".into(),
            outbound: "o".into(),
            process: "p".into(),
            inbound_user: "u".into(),
            upload: 1,
            download: 2,
        };
        let day = day_name(old_day);
        let (start, _) = day_bounds(&day).unwrap();
        write_records(&dir.join(format!("{day}.tsv")), &[rec(start + 60), rec(start + 120), rec(start + 3700)]).unwrap();
        maintain(&dir, now, DEFAULT_RETENTION_SECS);
        let folded = read_records(&dir.join(format!("{day}.h.tsv")));
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].download, 4);
        assert!(!dir.join(format!("{day}.tsv")).exists());
        maintain(&dir, now, 86400);
        assert!(files(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn series_fills_buckets() {
        let rec = |at: i64, down: u64| Record {
            at,
            source_ip: String::new(),
            host: String::new(),
            outbound: String::new(),
            process: String::new(),
            inbound_user: String::new(),
            upload: 0,
            download: down,
        };
        let s = series(&[rec(120, 5), rec(130, 5), rec(300, 1)], 60, 300, 60);
        assert_eq!(s.len(), 5);
        assert_eq!(s[1], (120, 0, 10));
        assert_eq!(s[4], (300, 0, 1));
    }
}
