//! The `surfman` build script.

use cfg_aliases::cfg_aliases;
use gl_generator::{Api, Fallbacks, Profile, Registry, StructGenerator};
use std::env;
use std::fs::File;
use std::path::PathBuf;

fn main() {
    // Detect Termux: an Android userspace that provides a full Mesa/EGL stack
    // but NOT the Android `AHardwareBuffer` API. On Termux we route to the
    // software `mesa_surfaceless` backend instead of `hardware_buffer`.
    //
    // These are emitted manually (with `cargo::rustc-cfg`) because the
    // `cfg_aliases!` macro below cannot read environment variables.
    let is_android = env::var("CARGO_CFG_TARGET_OS").map(|v| v == "android").unwrap_or(false);
    // Declare the custom cfgs so rustc's `unexpected_cfgs` lint is satisfied.
    println!("cargo::rustc-check-cfg=cfg(termux_android)");
    println!("cargo::rustc-check-cfg=cfg(surfaceless_platform)");
    println!("cargo::rustc-check-cfg=cfg(android_hardware_buffer)");
    if env::var("TERMUX_VERSION").is_ok() {
        // Termux: treat android as unix-like with Mesa software rendering.
        println!("cargo::rustc-cfg=termux_android");
        println!("cargo::rustc-cfg=surfaceless_platform");
    } else if is_android {
        // Real Android: hardware buffer backend is available.
        println!("cargo::rustc-cfg=android_hardware_buffer");
    }

    // Setup aliases for #[cfg] checks
    cfg_aliases! {
        // Platforms
        android_platform: { target_os = "android" },
        ohos_platform: { target_env = "ohos" },
        web_platform: { all(target_family = "wasm", target_os = "unknown") },
        macos_platform: { target_os = "macos" },
        ios_platform: { target_os = "ios" },
        windows_platform: { target_os = "windows" },
        apple: { any(target_os = "ios", target_os = "macos") },
        free_unix: { all(unix, not(apple), not(android_platform), not(target_os = "emscripten"), not(ohos_platform)) },

        // Native displays.
        x11_platform: { all(free_unix, feature = "sm-x11") },
        wayland_platform: { all(free_unix) },

        // Features:
        // Here we collect the features that are only valid on certain platforms and
        // we add aliases that include checks for the correct platform.
        angle: { all(windows, feature = "sm-angle") },
        angle_builtin: { all(windows_platform, feature = "sm-angle-builtin") },
        angle_default: { all(windows_platform, feature = "sm-angle-default") },
        no_wgl: { all(windows_platform, feature = "sm-no-wgl") },
        wayland_default: { all(wayland_platform, any(not(x11_platform), feature = "sm-wayland-default")) },
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").ok();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap();
    let dest = PathBuf::from(&env::var("OUT_DIR").unwrap());

    // Generate EGL bindings.
    if target_os == "android"
        || (target_os == "windows" && cfg!(feature = "sm-angle"))
        || target_env == "ohos"
        || target_family.as_ref().map_or(false, |f| f == "unix")
    {
        let mut file = File::create(dest.join("egl_bindings.rs")).unwrap();
        let registry = Registry::new(Api::Egl, (1, 5), Profile::Core, Fallbacks::All, []);
        registry.write_bindings(StructGenerator, &mut file).unwrap();
    }
}
