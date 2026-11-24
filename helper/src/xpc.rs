//! A small binding of XPC's C API (libxpc), shared by the helper and the
//! app: dictionaries, a Mach service listener and a client.
#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};

use block2::RcBlock;

pub type xpc_object_t = *mut c_void;
type xpc_type_t = *const c_void;

pub const XPC_CONNECTION_MACH_SERVICE_LISTENER: u64 = 1 << 0;
pub const XPC_CONNECTION_MACH_SERVICE_PRIVILEGED: u64 = 1 << 1;

unsafe extern "C" {
    static _xpc_type_connection: c_void;
    static _xpc_type_dictionary: c_void;
    static _xpc_type_error: c_void;
    static _xpc_error_key_description: *const c_char;

    fn xpc_connection_create(name: *const c_char, targetq: *mut c_void) -> xpc_object_t;
    fn xpc_connection_create_mach_service(name: *const c_char, targetq: *mut c_void, flags: u64) -> xpc_object_t;
    fn xpc_endpoint_create(connection: xpc_object_t) -> xpc_object_t;
    fn xpc_connection_create_from_endpoint(endpoint: xpc_object_t) -> xpc_object_t;
    fn xpc_connection_set_event_handler(connection: xpc_object_t, handler: *mut c_void);
    fn xpc_connection_set_peer_code_signing_requirement(connection: xpc_object_t, requirement: *const c_char) -> c_int;
    fn xpc_connection_resume(connection: xpc_object_t);
    fn xpc_connection_cancel(connection: xpc_object_t);
    fn xpc_connection_send_message(connection: xpc_object_t, message: xpc_object_t);
    fn xpc_connection_send_message_with_reply_sync(connection: xpc_object_t, message: xpc_object_t) -> xpc_object_t;
    fn xpc_get_type(object: xpc_object_t) -> xpc_type_t;
    fn xpc_dictionary_create(keys: *const *const c_char, values: *const xpc_object_t, count: usize) -> xpc_object_t;
    fn xpc_dictionary_create_reply(original: xpc_object_t) -> xpc_object_t;
    fn xpc_dictionary_get_remote_connection(dictionary: xpc_object_t) -> xpc_object_t;
    fn xpc_dictionary_set_string(dictionary: xpc_object_t, key: *const c_char, value: *const c_char);
    fn xpc_dictionary_set_int64(dictionary: xpc_object_t, key: *const c_char, value: i64);
    fn xpc_dictionary_set_bool(dictionary: xpc_object_t, key: *const c_char, value: bool);
    fn xpc_dictionary_get_string(dictionary: xpc_object_t, key: *const c_char) -> *const c_char;
    fn xpc_dictionary_get_int64(dictionary: xpc_object_t, key: *const c_char) -> i64;
    fn xpc_dictionary_get_bool(dictionary: xpc_object_t, key: *const c_char) -> bool;
    fn xpc_retain(object: xpc_object_t) -> xpc_object_t;
    fn xpc_release(object: xpc_object_t);
    pub fn dispatch_main() -> !;
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// An owned XPC object (released when dropped).
pub struct Object(xpc_object_t);

unsafe impl Send for Object {}

impl Drop for Object {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { xpc_release(self.0) };
        }
    }
}

impl Object {
    /// Takes `raw` (a +1 reference).
    unsafe fn owned(raw: xpc_object_t) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw))
    }

    /// Retains `raw` (a borrowed reference).
    unsafe fn retained(raw: xpc_object_t) -> Option<Self> {
        (!raw.is_null()).then(|| Self(unsafe { xpc_retain(raw) }))
    }

    pub fn raw(&self) -> xpc_object_t {
        self.0
    }

    pub fn dictionary() -> Self {
        Self(unsafe { xpc_dictionary_create(std::ptr::null(), std::ptr::null(), 0) })
    }

    pub fn is_dictionary(&self) -> bool {
        unsafe { xpc_get_type(self.0) == &_xpc_type_dictionary as *const c_void }
    }

    pub fn is_error(&self) -> bool {
        unsafe { xpc_get_type(self.0) == &_xpc_type_error as *const c_void }
    }

    fn is_connection(&self) -> bool {
        unsafe { xpc_get_type(self.0) == &_xpc_type_connection as *const c_void }
    }

    pub fn set_string(&self, key: &str, value: &str) {
        let (k, v) = (c(key), c(value));
        unsafe { xpc_dictionary_set_string(self.0, k.as_ptr(), v.as_ptr()) };
    }

    pub fn set_int(&self, key: &str, value: i64) {
        let k = c(key);
        unsafe { xpc_dictionary_set_int64(self.0, k.as_ptr(), value) };
    }

    pub fn set_bool(&self, key: &str, value: bool) {
        let k = c(key);
        unsafe { xpc_dictionary_set_bool(self.0, k.as_ptr(), value) };
    }

    pub fn string(&self, key: &str) -> Option<String> {
        let k = c(key);
        let p = unsafe { xpc_dictionary_get_string(self.0, k.as_ptr()) };
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    pub fn int(&self, key: &str) -> i64 {
        let k = c(key);
        unsafe { xpc_dictionary_get_int64(self.0, k.as_ptr()) }
    }

    pub fn bool(&self, key: &str) -> bool {
        let k = c(key);
        unsafe { xpc_dictionary_get_bool(self.0, k.as_ptr()) }
    }

    /// An error's description.
    pub fn error_description(&self) -> String {
        let p = unsafe { xpc_dictionary_get_string(self.0, _xpc_error_key_description) };
        if p.is_null() { "XPC error".into() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
    }

    /// The reply to this request.
    pub fn reply(&self) -> Option<Self> {
        unsafe { Self::owned(xpc_dictionary_create_reply(self.0)) }
    }
}

/// Serves requests: each new connection is checked against
/// `client_requirement` (when given), then its dictionaries are answered by
/// `handle` (request, reply).
fn serve(listener: xpc_object_t, client_requirement: Option<&str>, handle: fn(&Object, &Object)) {
    let requirement = client_requirement.map(c);
    let handler = RcBlock::new(move |event: xpc_object_t| {
        let Some(peer) = (unsafe { Object::retained(event) }) else { return };
        if !peer.is_connection() {
            return;
        }
        if let Some(req) = &requirement {
            // XPC refuses messages from clients not matching (macOS 12+).
            if unsafe { xpc_connection_set_peer_code_signing_requirement(peer.raw(), req.as_ptr()) } != 0 {
                unsafe { xpc_connection_cancel(peer.raw()) };
                return;
            }
        }
        let peer_raw = peer.raw();
        let peer_handler = RcBlock::new(move |message: xpc_object_t| {
            let Some(request) = (unsafe { Object::retained(message) }) else { return };
            if !request.is_dictionary() {
                return;
            }
            let Some(reply) = request.reply() else { return };
            handle(&request, &reply);
            unsafe {
                let remote = xpc_dictionary_get_remote_connection(request.raw());
                xpc_connection_send_message(remote, reply.raw());
            }
        });
        unsafe {
            xpc_connection_set_event_handler(peer_raw, (&*peer_handler as *const _ as *mut c_void).cast());
            xpc_connection_resume(peer_raw);
        }
        // The connection keeps itself alive while open.
        std::mem::forget(peer);
    });
    unsafe {
        xpc_connection_set_event_handler(listener, (&*handler as *const _ as *mut c_void).cast());
        xpc_connection_resume(listener);
    }
    std::mem::forget(handler);
}

/// Listens on Mach service `name` (a launchd job's) and never returns.
pub fn listen_mach_service(name: &str, client_requirement: &str, handle: fn(&Object, &Object)) -> ! {
    let n = c(name);
    let listener = unsafe { xpc_connection_create_mach_service(n.as_ptr(), std::ptr::null_mut(), XPC_CONNECTION_MACH_SERVICE_LISTENER) };
    serve(listener, Some(client_requirement), handle);
    unsafe { dispatch_main() }
}

/// An anonymous listener and an endpoint to it (tests: the same protocol
/// without launchd). Returns the listener, kept alive by the caller.
pub fn anonymous_listener(handle: fn(&Object, &Object)) -> (Object, Object) {
    let listener = unsafe { xpc_connection_create(std::ptr::null(), std::ptr::null_mut()) };
    serve(listener, None, handle);
    let endpoint = unsafe { xpc_endpoint_create(listener) };
    (Object(listener), Object(endpoint))
}

/// A client connection.
pub struct Client(Object);

impl Client {
    /// To the privileged Mach service `name`, which must match
    /// `service_requirement` when given.
    pub fn mach_service(name: &str, service_requirement: Option<&str>) -> Self {
        let n = c(name);
        let raw = unsafe { xpc_connection_create_mach_service(n.as_ptr(), std::ptr::null_mut(), XPC_CONNECTION_MACH_SERVICE_PRIVILEGED) };
        if let Some(req) = service_requirement {
            let r = c(req);
            unsafe { xpc_connection_set_peer_code_signing_requirement(raw, r.as_ptr()) };
        }
        Self::start(raw)
    }

    pub fn from_endpoint(endpoint: &Object) -> Self {
        Self::start(unsafe { xpc_connection_create_from_endpoint(endpoint.raw()) })
    }

    fn start(raw: xpc_object_t) -> Self {
        // Replies come synchronously; other events are ignored.
        let handler = RcBlock::new(|_event: xpc_object_t| {});
        unsafe {
            xpc_connection_set_event_handler(raw, (&*handler as *const _ as *mut c_void).cast());
            xpc_connection_resume(raw);
        }
        Self(Object(raw))
    }

    /// Sends `request` and waits for its reply (blocking).
    pub fn send(&self, request: &Object) -> Result<Object, String> {
        let raw = unsafe { xpc_connection_send_message_with_reply_sync(self.0.raw(), request.raw()) };
        let reply = unsafe { Object::owned(raw) }.ok_or("no reply")?;
        if reply.is_error() {
            return Err(reply.error_description());
        }
        Ok(reply)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe { xpc_connection_cancel(self.0.raw()) };
    }
}
