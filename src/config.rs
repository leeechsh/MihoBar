//! Config files (ClashBar's `ConfigDirectoryManager` and
//! `ConfigImportService`): the `.yaml` / `.yml` files of the config
//! directory, the selected one, and the helpers that import new ones.

use std::path::{Path, PathBuf};

use crate::paths::WorkingDirectory;

/// The default config shipped in the app, seeded as `Mihobar.yaml`.
pub const DEFAULT_CONFIG_NAME: &str = "Mihobar.yaml";
pub const DEFAULT_CONFIG: &str = include_str!("../assets/config/Mihobar.yaml");

/// The config files in `dir`, by name: regular files (links followed) with
/// a `yaml` / `yml` extension, hidden ones skipped.
pub fn scan_config_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                return false;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            (ext == "yaml" || ext == "yml") && std::fs::metadata(p).is_ok_and(|m| m.is_file())
        })
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files
}

/// Writes the default config into the config directory when missing.
pub fn seed_default_config(wd: &WorkingDirectory) -> Result<(), String> {
    let target = wd.config_dir().join(DEFAULT_CONFIG_NAME);
    if target.exists() {
        return Ok(());
    }
    write_config_data(DEFAULT_CONFIG.as_bytes(), &target).map(|_| ())
}

/// Writes `data` to `target` atomically; `Ok(false)` when the file already
/// holds it. Data that is no Clash config (no `proxies:`, `proxy-groups:`
/// or `proxy-providers:`) is refused.
pub fn write_config_data(data: &[u8], target: &Path) -> Result<bool, String> {
    if data.is_empty() {
        return Err("Remote config response is empty".into());
    }
    let text = std::str::from_utf8(data).map_err(|_| "Config is not UTF-8 text".to_string())?;
    if !(text.contains("proxies:") || text.contains("proxy-groups:") || text.contains("proxy-providers:")) {
        let preview: String = text.chars().take(200).collect();
        return Err(format!(
            "Remote config is not a valid Clash configuration (missing proxy keys). Content preview: {preview}"
        ));
    }
    if std::fs::read(target).is_ok_and(|existing| existing == data) {
        return Ok(false);
    }
    let tmp = target.with_extension("tmp-write");
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| e.to_string())?;
    Ok(true)
}

/// A file name for an imported config: its last path component, `.yaml`
/// added when it has no extension; `None` for `.`, `..`, empty names and
/// other extensions.
pub fn normalized_config_file_name(name: &str, fallback: Option<&str>) -> Option<String> {
    let trimmed = name.trim();
    let base = if trimmed.is_empty() { fallback.unwrap_or("") } else { trimmed };
    let candidate = Path::new(base).file_name()?.to_str()?.trim().to_owned();
    if candidate.is_empty() || candidate == "." || candidate == ".." {
        return None;
    }
    match Path::new(&candidate).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) {
        None => Some(format!("{candidate}.yaml")),
        Some(ext) if ext == "yaml" || ext == "yml" => Some(candidate),
        Some(_) => None,
    }
}

/// The first top-level scalar `key: value` in a YAML document, quotes and
/// trailing comments removed (ClashBar's `parseYAMLScalarValue`).
pub fn yaml_scalar(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        if line.starts_with(' ') || line.starts_with('\t') || line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix(key) else { continue };
        let Some(value) = rest.trim_start().strip_prefix(':') else { continue };
        let value = strip_comment(value).trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        return Some(value.to_owned());
    }
    None
}

fn strip_comment(value: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && (i == 0 || bytes[i - 1].is_ascii_whitespace()) => {
                return &value[..i];
            }
            _ => {}
        }
    }
    value
}

/// The controller address the core is launched with: the config's
/// `external-controller`, a wildcard host made local, or the default.
pub fn launch_controller(config_content: &str) -> String {
    const DEFAULT: &str = "127.0.0.1:9097";
    let Some(raw) = yaml_scalar(config_content, "external-controller") else {
        return DEFAULT.into();
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return DEFAULT.into();
    }
    if let Some(port) = raw.strip_prefix(':') {
        return format!("127.0.0.1:{port}");
    }
    raw.to_owned()
}

/// Where the API is reached for a launch controller: a wildcard host
/// (`0.0.0.0`, `*`, `[::]`) is reached on the loopback address.
pub fn api_host_port(controller: &str) -> String {
    let (host, port) = match controller.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => return controller.to_owned(),
    };
    let host = match host {
        "" | "0.0.0.0" | "*" | "[::]" | "::" => "127.0.0.1",
        other => other,
    };
    format!("{host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars() {
        let doc = "mixed-port: 7897\nexternal-controller: '127.0.0.1:9097' # api\nsecret: \"abc\"\ndns:\n  secret: no\n";
        assert_eq!(yaml_scalar(doc, "external-controller").as_deref(), Some("127.0.0.1:9097"));
        assert_eq!(yaml_scalar(doc, "secret").as_deref(), Some("abc"));
        assert_eq!(yaml_scalar(doc, "mode"), None);
        assert_eq!(launch_controller("external-controller: :9090\n"), "127.0.0.1:9090");
        assert_eq!(api_host_port("0.0.0.0:9090"), "127.0.0.1:9090");
    }

    #[test]
    fn names() {
        assert_eq!(normalized_config_file_name("a", None).as_deref(), Some("a.yaml"));
        assert_eq!(normalized_config_file_name("/x/b.yml", None).as_deref(), Some("b.yml"));
        assert_eq!(normalized_config_file_name("c.txt", None), None);
        assert_eq!(normalized_config_file_name(" ", Some("d")).as_deref(), Some("d.yaml"));
    }
}
