//! The Wi-Fi name and the location permission it needs (ClashBar's
//! `SSIDMonitorService`). AppKit-side calls: run them on the main thread.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};
use objc2_core_wlan::CWWiFiClient;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Authorization {
    #[default]
    NotDetermined,
    Authorized,
    Denied,
    Restricted,
    ServicesDisabled,
}

impl Authorization {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Authorized => "ui.settings.ssid_strategy.status.authorized",
            Self::NotDetermined => "ui.settings.ssid_strategy.status.not_determined",
            Self::Denied => "ui.settings.ssid_strategy.status.denied",
            Self::Restricted => "ui.settings.ssid_strategy.status.restricted",
            Self::ServicesDisabled => "ui.settings.ssid_strategy.status.services_disabled",
        }
    }
}

thread_local! {
    static MANAGER: RefCell<Option<Retained<CLLocationManager>>> = const { RefCell::new(None) };
}

fn with_manager<R>(f: impl FnOnce(&CLLocationManager) -> R) -> R {
    MANAGER.with_borrow_mut(|m| f(m.get_or_insert_with(|| unsafe { CLLocationManager::new() })))
}

fn authorization() -> Authorization {
    if !unsafe { CLLocationManager::locationServicesEnabled_class() } {
        return Authorization::ServicesDisabled;
    }
    match with_manager(|m| unsafe { m.authorizationStatus() }) {
        CLAuthorizationStatus::AuthorizedAlways | CLAuthorizationStatus::AuthorizedWhenInUse => Authorization::Authorized,
        CLAuthorizationStatus::Denied => Authorization::Denied,
        CLAuthorizationStatus::Restricted => Authorization::Restricted,
        _ => Authorization::NotDetermined,
    }
}

/// The permission and, when granted, the Wi-Fi joined.
pub fn snapshot() -> (Authorization, Option<String>) {
    let auth = authorization();
    let ssid = (auth == Authorization::Authorized)
        .then(|| unsafe { CWWiFiClient::sharedWiFiClient().interface()?.ssid() })
        .flatten()
        .map(|s| s.to_string().trim().to_owned())
        .filter(|s| !s.is_empty());
    (auth, ssid)
}

/// Asks for the location permission when it was never asked.
pub fn request_authorization() {
    if authorization() == Authorization::NotDetermined {
        with_manager(|m| unsafe { m.requestWhenInUseAuthorization() });
    }
}
