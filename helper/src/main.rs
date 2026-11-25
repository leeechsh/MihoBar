//! MihoBar's privileged helper (after the design of ClashBar's
//! `ProxyHelper`): a launchd daemon, run as root, that sets the macOS system proxy for the
//! app over XPC. Only `dev.mihobar.app` may connect.

mod protocol;
mod proxy;
mod xpc;

use protocol::{key, op};

/// Answers one request (`reply` gets `ok`, `error` and the values).
fn handle(request: &xpc::Object, reply: &xpc::Object) {
    let ports = || proxy::Ports {
        http: request.int(key::HTTP_PORT),
        https: request.int(key::HTTPS_PORT),
        socks: request.int(key::SOCKS_PORT),
    };
    let host = || request.string(key::HOST).unwrap_or_default();
    let result: proxy::Result<()> = match request.string(key::OP).as_deref() {
        Some(op::PING) => Ok(()),
        Some(op::SET_PROXY) => proxy::set_system_proxy(&host(), &ports()),
        Some(op::CLEAR_PROXY) => proxy::clear_system_proxy(),
        Some(op::GET_STATE) => proxy::is_system_proxy_enabled().map(|on| reply.set_bool(key::ENABLED, on)),
        Some(op::GET_ACTIVE_TARGET) => proxy::active_target().map(|target| {
            if let Some((h, p)) = target {
                reply.set_string(key::HOST, &h);
                reply.set_int(key::PORT, p);
            }
        }),
        Some(op::IS_CONFIGURED) => {
            proxy::is_system_proxy_configured(&host(), &ports()).map(|c| reply.set_bool(key::CONFIGURED, c))
        }
        Some(op::GET_EXCEPTIONS) => proxy::exceptions().map(|list| reply.set_string(key::EXCEPTIONS, &list.join("\n"))),
        Some(op::SET_EXCEPTIONS) => {
            let raw = request.string(key::EXCEPTIONS).unwrap_or_default();
            let list: Vec<String> = raw.lines().map(str::to_owned).collect();
            proxy::set_exceptions(&list)
        }
        other => Err(format!("Unknown request: {}", other.unwrap_or("-"))),
    };
    match result {
        Ok(()) => reply.set_bool(key::OK, true),
        Err(e) => {
            reply.set_bool(key::OK, false);
            reply.set_string(key::ERROR, &e);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") => println!("{}", protocol::HELPER_VERSION),
        Some("--print-plist") => print!("{}", protocol::launchd_plist()),
        _ => xpc::listen_mach_service(protocol::MACH_SERVICE_NAME, protocol::ALLOWED_CLIENT_REQUIREMENT, handle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole protocol over a real XPC connection (an anonymous one:
    /// no launchd, no root). Reads succeed; writes fail without root.
    #[test]
    fn answers_over_xpc() {
        let (_listener, endpoint) = xpc::anonymous_listener(handle);
        let client = xpc::Client::from_endpoint(&endpoint);
        let request = |op: &str| {
            let r = xpc::Object::dictionary();
            r.set_string(key::OP, op);
            r
        };
        let ping = client.send(&request(op::PING)).unwrap();
        assert!(ping.bool(key::OK));
        let state = client.send(&request(op::GET_STATE)).unwrap();
        assert!(state.bool(key::OK));
        let exceptions = client.send(&request(op::GET_EXCEPTIONS)).unwrap();
        assert!(exceptions.bool(key::OK));
        let unknown = client.send(&request("nope")).unwrap();
        assert!(!unknown.bool(key::OK));
        assert_eq!(unknown.string(key::ERROR).as_deref(), Some("Unknown request: nope"));
        let set = request(op::SET_PROXY);
        set.set_string(key::HOST, "127.0.0.1");
        set.set_int(key::HTTP_PORT, 7897);
        let reply = client.send(&set).unwrap();
        if unsafe { getuid() } != 0 {
            assert!(!reply.bool(key::OK));
            assert!(reply.string(key::ERROR).unwrap().contains("root"));
        }
    }

    unsafe extern "C" {
        fn getuid() -> u32;
    }
}
