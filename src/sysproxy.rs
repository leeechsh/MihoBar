//! The system proxy through the privileged helper (ClashBar's
//! `SystemProxyService`, `ProxyHelperInstaller`, `AdministratorShell`).
//! Every call blocks: the app makes them on blocking threads.

#[path = "../helper/src/protocol.rs"]
pub mod protocol;
#[path = "../helper/src/xpc.rs"]
pub mod xpc;

use std::path::{Path, PathBuf};
use std::process::Command;

use protocol::{key, op};

/// The ports the system proxy points at (ClashBar's `SystemProxyPorts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SystemProxyPorts {
    pub http: Option<i64>,
    pub https: Option<i64>,
    pub socks: Option<i64>,
}

impl SystemProxyPorts {
    /// The mixed port for all three, else the HTTP port for HTTP(S) and the
    /// SOCKS port (ClashBar's `resolveSystemProxyPorts`).
    pub fn resolve(mixed: Option<i64>, http: Option<i64>, socks: Option<i64>) -> Self {
        let valid = |v: Option<i64>| v.filter(|p| (1..=65535).contains(p));
        if let Some(m) = valid(mixed) {
            return Self { http: Some(m), https: Some(m), socks: Some(m) };
        }
        Self { http: valid(http), https: valid(http), socks: valid(socks) }
    }
    pub fn has_enabled_port(&self) -> bool {
        self.http.is_some() || self.https.is_some() || self.socks.is_some()
    }
    pub fn primary(&self) -> Option<i64> {
        self.http.or(self.https).or(self.socks)
    }
}

/// `127.0.0.1:7897` (`[::1]:7897` for IPv6), ClashBar's
/// `buildSystemProxyDisplayString`.
pub fn display(host: &str, ports: &SystemProxyPorts) -> Option<String> {
    let port = ports.primary().filter(|p| *p > 0)?;
    let host = host.trim();
    Some(if host.contains(':') && !host.starts_with('[') { format!("[{host}]:{port}") } else { format!("{host}:{port}") })
}

/// ClashBar's default bypass list.
pub const DEFAULT_EXCEPTIONS: [&str; 8] =
    ["::1", "*.local", "<local>", "localhost", "127.0.0.1", "192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidHost,
    InvalidPort,
    HelperNotBundled,
    RequiresApplications,
    HelperNotRegistered,
    HelperStartTimedOut,
    AuthorizationCancelled,
    InstallFailed(String),
    ConnectionFailed(String),
    OperationFailed(String),
}

impl Error {
    /// The localized message key and its argument (ClashBar's
    /// `systemProxyErrorMessage`).
    pub fn message_key(&self) -> (&'static str, Option<&str>) {
        match self {
            Self::InvalidHost => ("app.system_proxy.error.invalid_host", None),
            Self::InvalidPort => ("app.system_proxy.error.invalid_port", None),
            Self::HelperNotBundled => ("app.system_proxy.error.helper_not_bundled", None),
            Self::RequiresApplications => ("app.system_proxy.error.helper_install_location", None),
            Self::HelperNotRegistered => ("app.system_proxy.error.helper_not_registered", None),
            Self::HelperStartTimedOut => ("app.system_proxy.error.helper_start_timed_out", None),
            Self::AuthorizationCancelled => ("app.system_proxy.error.helper_authorization_cancelled", None),
            Self::InstallFailed(m) => ("app.system_proxy.error.helper_install_failed", Some(m)),
            Self::ConnectionFailed(m) => ("app.system_proxy.error.helper_connection_failed", Some(m)),
            Self::OperationFailed(m) => ("app.system_proxy.error.helper_operation_failed", Some(m)),
        }
    }
}

// --- Administrator shell -------------------------------------------------------

/// `'value'` for a shell.
pub fn shell_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn apple_script_escaped(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Runs `command` as root after the system's administrator prompt
/// (`osascript`'s `with administrator privileges`).
pub fn run_as_administrator(command: &str, cancelled: Error, failed: fn(String) -> Error) -> Result<(), Error> {
    let script = format!("do shell script \"{}\" with administrator privileges", apple_script_escaped(command));
    let output = Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| failed(e.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let message = [stderr, stdout].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| "Unknown authorization error.".into());
    if message.to_lowercase().contains("user canceled") || message.contains("-128") {
        return Err(cancelled);
    }
    Err(failed(message))
}

// --- Installer -------------------------------------------------------------------

/// The app bundle the executable is in.
pub fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    (contents.file_name()? == "Contents").then(|| contents.parent().map(Path::to_path_buf)).flatten()
}

fn has_quarantine(path: &str) -> bool {
    Command::new("/usr/bin/xattr").args(["-p", "com.apple.quarantine", path]).output().is_ok_and(|o| o.status.success())
}

/// The installed helper is this version's (ClashBar's `installedHelperIsCurrent`).
pub fn installed_helper_is_current() -> bool {
    let recorded = std::fs::read_to_string(protocol::INSTALLED_VERSION_PATH).unwrap_or_default();
    let executable = std::fs::metadata(protocol::INSTALLED_HELPER_PATH).is_ok_and(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o111 != 0
    });
    executable
        && Path::new(protocol::INSTALLED_PLIST_PATH).exists()
        && recorded.trim() == protocol::HELPER_VERSION.to_string()
        && !has_quarantine(protocol::INSTALLED_HELPER_PATH)
        && !has_quarantine(protocol::INSTALLED_PLIST_PATH)
}

/// The install script (ClashBar's `installShellCommand`).
pub fn install_command(source_binary: &str, source_plist: &str) -> String {
    let (binary, plist) = (shell_quoted(source_binary), shell_quoted(source_plist));
    let dest_binary = shell_quoted(protocol::INSTALLED_HELPER_PATH);
    let dest_plist = shell_quoted(protocol::INSTALLED_PLIST_PATH);
    let version_file = shell_quoted(protocol::INSTALLED_VERSION_PATH);
    let label = protocol::MACH_SERVICE_NAME;
    let version = protocol::HELPER_VERSION;
    format!(
        "set -e
/bin/mkdir -p /Library/PrivilegedHelperTools /Library/LaunchDaemons
/bin/rm -f {version_file}
/bin/launchctl bootout system/{label} >/dev/null 2>&1 || true
/bin/rm -f {dest_binary}
/bin/cp -X {binary} {dest_binary}
/usr/sbin/chown root:wheel {dest_binary}
/bin/chmod 755 {dest_binary}
/usr/bin/xattr -c {dest_binary} 2>/dev/null || true
/bin/rm -f {dest_plist}
/bin/cp -X {plist} {dest_plist}
/usr/sbin/chown root:wheel {dest_plist}
/bin/chmod 644 {dest_plist}
/usr/bin/xattr -c {dest_plist} 2>/dev/null || true
/bin/launchctl enable system/{label} >/dev/null 2>&1 || true
if ! /bin/launchctl bootstrap system {dest_plist} 2>/dev/null; then
    /bin/sleep 0.1
    /bin/launchctl bootout system/{label} >/dev/null 2>&1 || true
    /bin/launchctl bootstrap system {dest_plist}
fi
/bin/launchctl kickstart -k system/{label}
echo {version} > {version_file}"
    )
}

/// Installs the helper the bundle carries (an administrator prompt).
pub fn install_bundled_helper() -> Result<(), Error> {
    let bundle = bundle_path().ok_or(Error::HelperNotBundled)?;
    let binary = bundle.join(protocol::HELPER_BUNDLE_PROGRAM);
    let plist = bundle.join(protocol::HELPER_BUNDLE_PLIST);
    if !binary.exists() || !plist.exists() {
        return Err(Error::HelperNotBundled);
    }
    if !bundle.starts_with("/Applications") {
        return Err(Error::RequiresApplications);
    }
    let command = install_command(&binary.display().to_string(), &plist.display().to_string());
    run_as_administrator(&command, Error::AuthorizationCancelled, Error::InstallFailed)
}

// --- The helper's requests -----------------------------------------------------------

fn client() -> xpc::Client {
    xpc::Client::mach_service(protocol::MACH_SERVICE_NAME, Some(protocol::HELPER_REQUIREMENT))
}

fn call(op_name: &str, fill: impl FnOnce(&xpc::Object)) -> Result<xpc::Object, Error> {
    let request = xpc::Object::dictionary();
    request.set_string(key::OP, op_name);
    fill(&request);
    let reply = client().send(&request).map_err(Error::ConnectionFailed)?;
    if reply.bool(key::OK) {
        Ok(reply)
    } else {
        Err(Error::OperationFailed(reply.string(key::ERROR).unwrap_or_else(|| "unknown".into())))
    }
}

/// The helper installed and answering; installed first when asked
/// (ClashBar's `ensureHelperReadyForUse`).
pub fn ensure_helper_ready(install_if_needed: bool) -> Result<(), Error> {
    if !installed_helper_is_current() {
        if !install_if_needed {
            return Err(Error::HelperNotRegistered);
        }
        install_bundled_helper()?;
    }
    // launchd starts it on the first message; a few tries while it does.
    let mut last = Error::HelperStartTimedOut;
    for _ in 0..4 {
        match call(op::PING, |_| {}) {
            Ok(_) => return Ok(()),
            Err(e) => last = e,
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Err(match last {
        Error::ConnectionFailed(_) => Error::HelperStartTimedOut,
        other => other,
    })
}

fn port_args(r: &xpc::Object, ports: &SystemProxyPorts) {
    r.set_int(key::HTTP_PORT, ports.http.unwrap_or(0));
    r.set_int(key::HTTPS_PORT, ports.https.unwrap_or(0));
    r.set_int(key::SOCKS_PORT, ports.socks.unwrap_or(0));
}

/// Turns the system proxy on at `host:ports`, or off.
pub fn apply(enabled: bool, host: &str, ports: &SystemProxyPorts) -> Result<(), Error> {
    if enabled {
        if host.trim().is_empty() {
            return Err(Error::InvalidHost);
        }
        if !ports.has_enabled_port() {
            return Err(Error::InvalidPort);
        }
    }
    ensure_helper_ready(true)?;
    if enabled {
        call(op::SET_PROXY, |r| {
            r.set_string(key::HOST, host);
            port_args(r, ports);
        })
        .map(|_| ())
    } else {
        call(op::CLEAR_PROXY, |_| {}).map(|_| ())
    }
}

pub fn is_enabled() -> Result<bool, Error> {
    ensure_helper_ready(false)?;
    Ok(call(op::GET_STATE, |_| {})?.bool(key::ENABLED))
}

pub fn active_display() -> Result<Option<String>, Error> {
    ensure_helper_ready(false)?;
    let reply = call(op::GET_ACTIVE_TARGET, |_| {})?;
    Ok(reply.string(key::HOST).map(|h| {
        let port = reply.int(key::PORT);
        if h.contains(':') && !h.starts_with('[') { format!("[{h}]:{port}") } else { format!("{h}:{port}") }
    }))
}

pub fn is_configured(host: &str, ports: &SystemProxyPorts) -> Result<bool, Error> {
    ensure_helper_ready(false)?;
    Ok(call(op::IS_CONFIGURED, |r| {
        r.set_string(key::HOST, host);
        port_args(r, ports);
    })?
    .bool(key::CONFIGURED))
}

pub fn read_exceptions() -> Result<Vec<String>, Error> {
    ensure_helper_ready(false)?;
    let raw = call(op::GET_EXCEPTIONS, |_| {})?.string(key::EXCEPTIONS).unwrap_or_default();
    Ok(raw.lines().map(str::to_owned).filter(|s| !s.trim().is_empty()).collect())
}

pub fn set_exceptions(list: &[String]) -> Result<(), Error> {
    ensure_helper_ready(true)?;
    call(op::SET_EXCEPTIONS, |r| r.set_string(key::EXCEPTIONS, &list.join("\n"))).map(|_| ())
}

/// Turns the system proxy off at quit, waiting at most `timeout`
/// (ClashBar's `clearBlocking`).
pub fn clear_blocking(timeout: std::time::Duration) {
    if !installed_helper_is_current() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(call(op::CLEAR_PROXY, |_| {}));
    });
    let _ = rx.recv_timeout(timeout);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_and_quoting() {
        let p = SystemProxyPorts::resolve(Some(7897), Some(1), Some(2));
        assert_eq!(p, SystemProxyPorts { http: Some(7897), https: Some(7897), socks: Some(7897) });
        let p = SystemProxyPorts::resolve(Some(0), Some(2333), Some(2334));
        assert_eq!((p.http, p.socks), (Some(2333), Some(2334)));
        assert_eq!(display("::1", &p).as_deref(), Some("[::1]:2333"));
        assert_eq!(shell_quoted("a'b"), "'a'\"'\"'b'");
        assert_eq!(apple_script_escaped(r#"a"b\c"#), r#"a\"b\\c"#);
        let script = install_command("/Applications/MihoBar.app/x", "/Applications/MihoBar.app/y");
        assert!(script.contains("/bin/launchctl bootstrap system '/Library/LaunchDaemons/dev.mihobar.helper.plist'"));
        assert!(script.ends_with("echo 1 > '/Library/PrivilegedHelperTools/dev.mihobar.helper.version'"));
    }

    #[test]
    fn helper_not_installed_here() {
        // On a Mac without MihoBar's helper: not ready, nothing installed.
        if !installed_helper_is_current() {
            assert_eq!(ensure_helper_ready(false), Err(Error::HelperNotRegistered));
            assert_eq!(is_enabled(), Err(Error::HelperNotRegistered));
        }
    }
}
