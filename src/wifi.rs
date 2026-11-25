//! The Wi-Fi joined (ClashBar's `SSIDMonitorService`): CoreWLAN, which
//! NSH has no counterpart for. The location permission it needs is NSH's
//! (`nsh_extra::location`, see `wifi_types`). Run on the main thread.

use objc2_core_wlan::CWWiFiClient;

/// The Wi-Fi joined, once the location permission allows reading it.
pub fn current_ssid() -> Option<String> {
    unsafe { CWWiFiClient::sharedWiFiClient().interface()?.ssid() }
        .map(|s| s.to_string().trim().to_owned())
        .filter(|s| !s.is_empty())
}
