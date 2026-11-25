//! Runs MihoBar on macOS's native backend.

#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;

#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<mihobar::App>()),
        platform::AppOptions {
            title: "MihoBar".to_owned(),
            app_id: "dev.mihobar.app".to_owned(),
            size: (360, 640),
            activation_policy: platform::ActivationPolicy::Accessory,
            keep_alive_on_close: true,
            ..Default::default()
        },
    )
}

#[cfg(not(target_os = "macos"))]
compile_error!("MihoBar runs on macOS only.");
