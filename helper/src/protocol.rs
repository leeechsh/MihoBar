#![allow(dead_code)]
//! What the app and the helper share (ClashBar's `ProxyHelperShared`):
//! names, paths and the message keys of the XPC protocol. Messages are
//! XPC dictionaries: `op` names the request; replies carry `ok` and, on
//! failure, `error`.

pub const MACH_SERVICE_NAME: &str = "dev.mihobar.helper";
pub const DAEMON_PLIST_NAME: &str = "dev.mihobar.helper.plist";
pub const HELPER_BUNDLE_PROGRAM: &str = "Contents/Library/HelperTools/dev.mihobar.helper";
pub const HELPER_BUNDLE_PLIST: &str = "Contents/Library/LaunchDaemons/dev.mihobar.helper.plist";
pub const INSTALLED_HELPER_PATH: &str = "/Library/PrivilegedHelperTools/dev.mihobar.helper";
pub const INSTALLED_PLIST_PATH: &str = "/Library/LaunchDaemons/dev.mihobar.helper.plist";
pub const INSTALLED_VERSION_PATH: &str = "/Library/PrivilegedHelperTools/dev.mihobar.helper.version";
pub const HELPER_VERSION: u32 = 1;
/// Only the app may talk to the helper (checked by XPC on each connection).
pub const ALLOWED_CLIENT_REQUIREMENT: &str = "identifier \"dev.mihobar.app\"";
/// The app only talks to the helper.
pub const HELPER_REQUIREMENT: &str = "identifier \"dev.mihobar.helper\"";

pub mod op {
    pub const PING: &str = "ping";
    pub const SET_PROXY: &str = "set_proxy";
    pub const CLEAR_PROXY: &str = "clear_proxy";
    pub const GET_STATE: &str = "get_state";
    pub const GET_ACTIVE_TARGET: &str = "get_active_target";
    pub const IS_CONFIGURED: &str = "is_configured";
    pub const GET_EXCEPTIONS: &str = "get_exceptions";
    pub const SET_EXCEPTIONS: &str = "set_exceptions";
}

pub mod key {
    pub const OP: &str = "op";
    pub const OK: &str = "ok";
    pub const ERROR: &str = "error";
    pub const HOST: &str = "host";
    pub const HTTP_PORT: &str = "http_port";
    pub const HTTPS_PORT: &str = "https_port";
    pub const SOCKS_PORT: &str = "socks_port";
    pub const ENABLED: &str = "enabled";
    pub const CONFIGURED: &str = "configured";
    pub const PORT: &str = "port";
    /// Newline-separated, as ClashBar serializes them.
    pub const EXCEPTIONS: &str = "exceptions";
}

/// The launchd job (ClashBar's `com.clashbar.helper.plist`).
pub fn launchd_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{MACH_SERVICE_NAME}</string>
    <key>MachServices</key>
    <dict>
        <key>{MACH_SERVICE_NAME}</key>
        <true/>
    </dict>
    <key>Program</key>
    <string>{INSTALLED_HELPER_PATH}</string>
</dict>
</plist>
"#
    )
}
