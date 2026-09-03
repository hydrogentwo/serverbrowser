# Vendored surfman 0.13.0

This is a vendored copy of the `surfman` crate (v0.13.0) with a small patch to
run on Termux/Android.

Servo's `SoftwareRenderingContext` uses surfman's `default` backend. On
`target_os = "android"`, surfman selects its `hardware_buffer` backend, which
requires the Android `AHardwareBuffer` API and the
`EGL_ANDROID_get_native_client_buffer` EGL extension. Mesa on Termux does not
provide these, so rendering panics or fails to produce pixels.

## The patch

`build.rs` detects Termux (via the `TERMUX_VERSION` environment variable) and
emits a `termux_android` cfg, plus `surfaceless_platform`. `src/lib.rs` then:

1. Gates the `hardware_buffer` module on `android_hardware_buffer` (real Android
   / OpenHarmony only, not Termux).
2. Selects the `mesa_surfaceless` backend as `default` on Termux. This backend
   uses `EGL_PLATFORM_SURFACELESS_MESA`, i.e. Mesa's headless software rendering
   (llvmpipe), which works without any display server.

The upstream source is available at <https://github.com/servo/surfman>.

The only files changed relative to upstream are `build.rs` and `src/lib.rs`.