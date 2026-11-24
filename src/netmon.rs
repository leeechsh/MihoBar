//! Network reachability (ClashBar's `NetworkReachabilityMonitor`):
//! NWPathMonitor from the Network framework's C API.
#![allow(non_camel_case_types)]

use std::ffi::c_void;

use block2::RcBlock;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

type nw_path_monitor_t = *mut c_void;
type nw_path_t = *mut c_void;

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_path_monitor_create() -> nw_path_monitor_t;
    fn nw_path_monitor_set_queue(monitor: nw_path_monitor_t, queue: *mut c_void);
    fn nw_path_monitor_set_update_handler(monitor: nw_path_monitor_t, handler: *mut c_void);
    fn nw_path_monitor_start(monitor: nw_path_monitor_t);
    fn nw_path_get_status(path: nw_path_t) -> i32;
}

unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
}

/// `nw_path_status_satisfied`.
const SATISFIED: i32 = 1;

/// Starts watching the network: `true` when a path is usable. Runs for the
/// app's life.
pub fn watch() -> UnboundedReceiver<bool> {
    let (tx, rx) = unbounded_channel();
    let handler = RcBlock::new(move |path: nw_path_t| {
        let online = unsafe { nw_path_get_status(path) } == SATISFIED;
        let _ = tx.send(online);
    });
    unsafe {
        let monitor = nw_path_monitor_create();
        // QOS_CLASS_UTILITY.
        nw_path_monitor_set_queue(monitor, dispatch_get_global_queue(0x11, 0));
        nw_path_monitor_set_update_handler(monitor, (&*handler as *const _ as *mut c_void).cast());
        nw_path_monitor_start(monitor);
    }
    std::mem::forget(handler);
    rx
}
