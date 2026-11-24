//! Number formatting (ClashBar's `Formatters` and `compactMenuBarRate`).

/// The menu bar's rate: `0K`, `9.99K`, `99.9K`, `999K`, `1.00M`...
pub fn compact_menu_bar_rate(bytes_per_second: i64) -> String {
    let bytes = bytes_per_second.max(0);
    if bytes == 0 {
        return "0K".into();
    }
    let mut value = bytes as f64 / 1024.0;
    let units = ["K", "M", "G", "T"];
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let shown = value.min(999.0);
    if value < 10.0 {
        format!("{shown:.2}{}", units[unit])
    } else if value < 100.0 {
        format!("{shown:.1}{}", units[unit])
    } else {
        format!("{shown:.0}{}", units[unit])
    }
}

/// `12.34 KB/s`, `1.20 MB/s`.
pub fn speed(value: i64) -> String {
    let v = value.max(0);
    if v >= 1024 * 1024 {
        format!("{:.2} MB/s", v as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} KB/s", v as f64 / 1024.0)
    }
}

/// `12.3 MB`, `456.7 KB`.
pub fn memory(value: i64) -> String {
    let v = value.max(0);
    if v >= 1024 * 1024 {
        format!("{:.1} MB", v as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", v as f64 / 1024.0)
    }
}

fn rounded_bytes(value: i64, divisor: f64, unit: &str) -> String {
    format!("{} {unit}", (value as f64 / divisor).round() as i64)
}

/// `0 KB`, `1 KB`, `12 MB` (ClashBar's `bytesInteger`).
pub fn bytes_integer(value: i64) -> String {
    let v = value.max(0);
    let (kb, mb, gb, tb) = (1024i64, 1024i64.pow(2), 1024i64.pow(3), 1024i64.pow(4));
    if v >= tb {
        rounded_bytes(v, tb as f64, "TB")
    } else if v >= gb {
        rounded_bytes(v, gb as f64, "GB")
    } else if v >= mb {
        rounded_bytes(v, mb as f64, "MB")
    } else if v >= kb {
        rounded_bytes(v, kb as f64, "KB")
    } else if v == 0 {
        "0 KB".into()
    } else {
        "1 KB".into()
    }
}

/// `12.3 MB` (ClashBar's `bytesCompact`).
pub fn bytes_compact(value: i64) -> String {
    let v = value.max(0) as f64;
    if v >= 1024f64.powi(3) {
        format!("{:.1} GB", v / 1024f64.powi(3))
    } else if v >= 1024f64.powi(2) {
        format!("{:.1} MB", v / 1024f64.powi(2))
    } else {
        format!("{:.1} KB", v / 1024.0)
    }
}

fn compact_no_space(value: f64, unit: &str) -> String {
    if (value.round() - value).abs() < 0.05 {
        format!("{}{unit}", value.round() as i64)
    } else {
        format!("{value:.1}{unit}")
    }
}

/// `12GB`, `1.5TB` (ClashBar's `bytesCompactNoSpace`).
pub fn bytes_compact_no_space(value: i64) -> String {
    let v = value.max(0) as f64;
    if v >= 1024f64.powi(4) {
        compact_no_space(v / 1024f64.powi(4), "TB")
    } else if v >= 1024f64.powi(3) {
        compact_no_space(v / 1024f64.powi(3), "GB")
    } else if v >= 1024f64.powi(2) {
        compact_no_space(v / 1024f64.powi(2), "MB")
    } else {
        compact_no_space(v / 1024.0, "KB")
    }
}

/// `1.20 MB/s · 3.4 GB`.
pub fn speed_and_total(rate: i64, total: Option<i64>) -> String {
    format!("{} · {}", speed(rate), total.map(bytes_compact).unwrap_or_else(|| "--".into()))
}

/// Seconds since the epoch of an RFC 3339 / ISO 8601 date-time
/// (`2026-11-24T10:00:00.123+08:00`, `...Z`).
pub fn parse_iso8601(input: &str) -> Option<i64> {
    let s = input.trim();
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, mo, da): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let (time, offset_secs) = if let Some(t) = rest.strip_suffix('Z') {
        (t, 0)
    } else if let Some(i) = rest.rfind(['+', '-']).filter(|&i| i >= 8) {
        let (t, off) = rest.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let mut parts = off[1..].split(':');
        let oh: i64 = parts.next()?.parse().ok()?;
        let om: i64 = parts.next().unwrap_or("0").parse().ok()?;
        (t, sign * (oh * 3600 + om * 60))
    } else {
        (rest, 0)
    };
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let sec: f64 = t.next().unwrap_or("0").parse().ok()?;
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + sec as i64 - offset_secs)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// What a relative-time or expiry string says, for the caller to localize.
#[derive(Debug, PartialEq, Eq)]
pub enum Relative {
    Unknown,
    Minutes(i64),
    Hours(i64),
    Days(i64),
}

/// How long ago `input` was (ClashBar's `relativeTime`).
pub fn relative_time(input: Option<&str>, now: i64) -> Relative {
    let Some(at) = input.and_then(parse_iso8601) else { return Relative::Unknown };
    let interval = (now - at).max(0);
    if interval / 60 < 60 {
        Relative::Minutes(interval / 60)
    } else if interval / 3600 < 24 {
        Relative::Hours(interval / 3600)
    } else {
        Relative::Days(interval / 86400)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Expiry {
    Unknown,
    LongTerm,
    Expired,
    Today,
    Days(i64),
}

/// A subscription's expiry (ClashBar's `daysUntilExpiryShort`).
pub fn expiry(unix_seconds: Option<i64>, now: i64) -> Expiry {
    match unix_seconds {
        None => Expiry::Unknown,
        Some(0) => Expiry::LongTerm,
        Some(s) if s < 0 => Expiry::Unknown,
        Some(s) if s < now => Expiry::Expired,
        Some(s) => match (s - now) / 86400 {
            0 => Expiry::Today,
            d => Expiry::Days(d),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_rate() {
        assert_eq!(compact_menu_bar_rate(0), "0K");
        assert_eq!(compact_menu_bar_rate(1024), "1.00K");
        assert_eq!(compact_menu_bar_rate(50 * 1024), "50.0K");
        assert_eq!(compact_menu_bar_rate(500 * 1024), "500K");
        assert_eq!(compact_menu_bar_rate(2 * 1024 * 1024), "2.00M");
    }

    #[test]
    fn bytes_and_dates() {
        assert_eq!(bytes_integer(0), "0 KB");
        assert_eq!(bytes_integer(30 * 1024 * 1024), "30 MB");
        assert_eq!(bytes_compact_no_space(1536 * 1024 * 1024), "1.5GB");
        assert_eq!(parse_iso8601("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(parse_iso8601("2026-11-24T08:00:00.5+08:00"), parse_iso8601("2026-11-24T00:00:00Z"));
        assert_eq!(relative_time(Some("1970-01-01T00:00:00Z"), 7200), Relative::Hours(2));
        assert_eq!(expiry(Some(0), 5), Expiry::LongTerm);
        assert_eq!(expiry(Some(86400 * 3 + 10), 0), Expiry::Days(3));
    }
}
