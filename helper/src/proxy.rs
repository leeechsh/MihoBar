//! The system proxy in SystemConfiguration (ClashBar's
//! `SystemProxyConfigurator`): HTTP, HTTPS and SOCKS entries and the
//! exceptions of every enabled network service.

use objc2_core_foundation::{CFArray, CFDictionary, CFMutableDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_system_configuration::{
    SCNetworkProtocol, SCNetworkService, SCNetworkSet, SCPreferences, kSCNetworkProtocolTypeProxies,
    kSCPropNetProxiesExceptionsList, kSCPropNetProxiesExcludeSimpleHostnames, kSCPropNetProxiesHTTPEnable,
    kSCPropNetProxiesHTTPPort, kSCPropNetProxiesHTTPProxy, kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort,
    kSCPropNetProxiesHTTPSProxy, kSCPropNetProxiesSOCKSEnable, kSCPropNetProxiesSOCKSPort, kSCPropNetProxiesSOCKSProxy,
};

pub type Result<T> = std::result::Result<T, String>;

struct Entry {
    enable: &'static CFString,
    host: &'static CFString,
    port: &'static CFString,
}

fn entries() -> [Entry; 3] {
    unsafe {
        [
            Entry { enable: kSCPropNetProxiesHTTPEnable, host: kSCPropNetProxiesHTTPProxy, port: kSCPropNetProxiesHTTPPort },
            Entry { enable: kSCPropNetProxiesHTTPSEnable, host: kSCPropNetProxiesHTTPSProxy, port: kSCPropNetProxiesHTTPSPort },
            Entry { enable: kSCPropNetProxiesSOCKSEnable, host: kSCPropNetProxiesSOCKSProxy, port: kSCPropNetProxiesSOCKSPort },
        ]
    }
}

pub struct Ports {
    pub http: i64,
    pub https: i64,
    pub socks: i64,
}

impl Ports {
    fn as_array(&self) -> [i64; 3] {
        [self.http, self.https, self.socks]
    }
}

fn validate_host(host: &str) -> Result<()> {
    if host.trim().is_empty() { Err("Invalid proxy host.".into()) } else { Ok(()) }
}

fn validate_ports(p: &Ports, requires_enabled: bool) -> Result<()> {
    if p.as_array().iter().any(|v| !(0..=65535).contains(v)) {
        return Err("Invalid proxy port.".into());
    }
    if requires_enabled && p.as_array().iter().all(|v| *v == 0) {
        return Err("Invalid proxy port.".into());
    }
    Ok(())
}

fn preferences() -> Result<CFRetained<SCPreferences>> {
    SCPreferences::new(None, &CFString::from_static_str("dev.mihobar.helper"), None)
        .ok_or_else(|| "Unable to open SystemConfiguration preferences.".to_owned())
}

/// The proxies protocol of every enabled service of the current set.
fn proxy_protocols(prefs: &SCPreferences) -> Result<Vec<CFRetained<SCNetworkProtocol>>> {
    let set = SCNetworkSet::current(prefs).ok_or("No current network set.")?;
    let services = set.services().ok_or("No network services.")?;
    let services: CFRetained<CFArray<SCNetworkService>> = unsafe { CFRetained::cast_unchecked(services) };
    let kind = unsafe { kSCNetworkProtocolTypeProxies };
    Ok((0..services.len())
        .filter_map(|i| services.get(i))
        .filter(|s| s.enabled())
        .filter_map(|s| s.protocol(kind))
        .collect())
}

type Config = CFRetained<CFMutableDictionary<CFString, CFType>>;

/// A protocol's configuration, as a mutable copy.
fn configuration(protocol: &SCNetworkProtocol) -> Config {
    let copy = CFMutableDictionary::<CFString, CFType>::with_capacity(0);
    if let Some(config) = protocol.configuration() {
        let config: CFRetained<CFDictionary<CFString, CFType>> = unsafe { CFRetained::cast_unchecked(config) };
        let (keys, values) = config.to_vecs();
        for (k, v) in keys.iter().zip(values.iter()) {
            copy.set(k, v);
        }
    }
    copy
}

fn int_value(config: &Config, key: &CFString) -> Option<i64> {
    let value = config.get(key)?;
    value.downcast::<CFNumber>().ok().and_then(|n| n.as_i64())
}

fn string_value(config: &Config, key: &CFString) -> Option<String> {
    let value = config.get(key)?;
    value.downcast::<CFString>().ok().map(|s| s.to_string())
}

fn is_enabled(config: &Config, key: &CFString) -> bool {
    int_value(config, key).unwrap_or(0) != 0
}

fn set_entry(config: &Config, entry: &Entry, host: &str, port: i64) {
    let (enable, host, port) = if port > 0 { (1, host, port) } else { (0, "", 0) };
    config.set(entry.enable, &CFNumber::new_i64(enable));
    config.set(entry.host, &CFString::from_str(host));
    config.set(entry.port, &CFNumber::new_i64(port));
}

/// Locks, changes, commits and applies the preferences.
fn with_mutable_protocols(update: impl Fn(&[CFRetained<SCNetworkProtocol>]) -> Result<()>) -> Result<()> {
    let prefs = preferences()?;
    if !prefs.lock(true) {
        return Err("Lock SystemConfiguration preferences failed (the helper must run as root).".into());
    }
    let result = (|| {
        let protocols = proxy_protocols(&prefs)?;
        update(&protocols)?;
        if !prefs.commit_changes() {
            return Err("Commit SystemConfiguration changes failed.".into());
        }
        if !prefs.apply_changes() {
            return Err("Apply SystemConfiguration changes failed.".into());
        }
        Ok(())
    })();
    prefs.unlock();
    result
}

fn store(protocol: &SCNetworkProtocol, config: &Config, action: &str) -> Result<()> {
    let typed: &CFDictionary<CFString, CFType> = config;
    let dict: &CFDictionary = typed.as_ref();
    if unsafe { protocol.set_configuration(Some(dict)) } {
        Ok(())
    } else {
        Err(format!("{action} failed."))
    }
}

pub fn set_system_proxy(host: &str, ports: &Ports) -> Result<()> {
    validate_host(host)?;
    validate_ports(ports, true)?;
    with_mutable_protocols(|protocols| {
        for p in protocols {
            let config = configuration(p);
            for (entry, port) in entries().iter().zip(ports.as_array()) {
                set_entry(&config, entry, host, port);
            }
            store(p, &config, "Set proxy configuration")?;
        }
        Ok(())
    })
}

pub fn clear_system_proxy() -> Result<()> {
    with_mutable_protocols(|protocols| {
        for p in protocols {
            let config = configuration(p);
            for entry in entries().iter() {
                set_entry(&config, entry, "", 0);
            }
            store(p, &config, "Clear proxy configuration")?;
        }
        Ok(())
    })
}

pub fn is_system_proxy_enabled() -> Result<bool> {
    let prefs = preferences()?;
    Ok(proxy_protocols(&prefs)?.iter().any(|p| {
        let config = configuration(p);
        entries().iter().any(|e| is_enabled(&config, e.enable))
    }))
}

/// Whether every enabled service carries the expected entries.
pub fn is_system_proxy_configured(host: &str, ports: &Ports) -> Result<bool> {
    validate_host(host)?;
    validate_ports(ports, true)?;
    let prefs = preferences()?;
    for p in proxy_protocols(&prefs)? {
        let config = configuration(&p);
        for (entry, expected) in entries().iter().zip(ports.as_array()) {
            let enabled = is_enabled(&config, entry.enable);
            let matches = if expected > 0 {
                enabled
                    && string_value(&config, entry.host).is_some_and(|h| h.trim().eq_ignore_ascii_case(host.trim()))
                    && int_value(&config, entry.port) == Some(expected)
            } else {
                !enabled
            };
            if !matches {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The first enabled entry's host and port.
pub fn active_target() -> Result<Option<(String, i64)>> {
    let prefs = preferences()?;
    for p in proxy_protocols(&prefs)? {
        let config = configuration(&p);
        for entry in entries().iter() {
            if is_enabled(&config, entry.enable) {
                if let (Some(host), Some(port)) = (string_value(&config, entry.host), int_value(&config, entry.port)) {
                    if !host.trim().is_empty() && port > 0 {
                        return Ok(Some((host, port)));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Exceptions trimmed, empty ones and repeats (any case) dropped.
pub fn normalized_exceptions(values: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .iter()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && seen.insert(v.to_lowercase()))
        .collect()
}

pub fn exceptions() -> Result<Vec<String>> {
    let prefs = preferences()?;
    let key = unsafe { kSCPropNetProxiesExceptionsList };
    for p in proxy_protocols(&prefs)? {
        let config = configuration(&p);
        if let Some(value) = config.get(key) {
            if let Ok(list) = value.downcast::<CFArray>() {
                let list: CFRetained<CFArray<CFString>> = unsafe { CFRetained::cast_unchecked(list) };
                let values: Vec<String> = (0..list.len()).filter_map(|i| list.get(i)).map(|s| s.to_string()).collect();
                return Ok(normalized_exceptions(&values));
            }
        }
    }
    Ok(Vec::new())
}

pub fn set_exceptions(values: &[String]) -> Result<()> {
    let normalized = normalized_exceptions(values);
    let strings: Vec<CFRetained<CFString>> = normalized.iter().map(|s| CFString::from_str(s)).collect();
    let list = CFArray::from_retained_objects(&strings);
    with_mutable_protocols(|protocols| {
        for p in protocols {
            let config = configuration(p);
            unsafe {
                let value: &CFType = list.as_ref();
                config.set(kSCPropNetProxiesExceptionsList, value);
                config.set(kSCPropNetProxiesExcludeSimpleHostnames, &CFNumber::new_i64(0));
            }
            store(p, &config, "Set proxy exceptions")?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_without_root() {
        // Reading works for any user; this Mac's current state is whatever
        // it is, so only success is checked.
        assert!(is_system_proxy_enabled().is_ok());
        assert!(active_target().is_ok());
        assert!(exceptions().is_ok());
    }

    #[test]
    fn writes_need_root() {
        if unsafe { libc_getuid() } == 0 {
            return;
        }
        let err = set_system_proxy("127.0.0.1", &Ports { http: 1, https: 1, socks: 1 }).unwrap_err();
        assert!(err.contains("root"), "{err}");
    }

    #[test]
    fn validates() {
        assert!(set_system_proxy(" ", &Ports { http: 1, https: 0, socks: 0 }).is_err());
        assert!(validate_ports(&Ports { http: 0, https: 0, socks: 0 }, true).is_err());
        assert!(validate_ports(&Ports { http: 70000, https: 0, socks: 0 }, false).is_err());
        assert_eq!(normalized_exceptions(&[" a ".into(), "A".into(), "".into(), "b".into()]), vec!["a", "b"]);
    }

    unsafe extern "C" {
        #[link_name = "getuid"]
        fn libc_getuid() -> u32;
    }
}
