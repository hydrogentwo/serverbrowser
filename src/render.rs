//! Servo rendering: navigate to a URL and produce RGBA pixels using Servo's
//! software (headless) rendering path.
//!
//! The pattern follows Servo's own integration tests and the `servo-shot`
//! reference implementation (proven to work headless):
//!
//!   1. Build a `SoftwareRenderingContext` and `make_current()`.
//!   2. Build `Servo` with empty-proxy preferences (avoids proxy stalls).
//!   3. `WebViewDelegate::notify_new_frame_ready` MUST call `webview.paint()`
//!      — that is the contract that fills the framebuffer; without it you get
//!      an all-white buffer.
//!   4. Wait for `LoadStatus::Complete`, force one more frame via a rAF nudge,
//!      wait for the post-load frame, then read pixels with
//!      `RenderingContext::read_to_image` (NOT `present`, which wipes the
//!      SoftwareRenderingContext back buffer).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use dpi::PhysicalSize;
use servo::{
    LoadStatus, Preferences, RenderingContext, Servo, ServoBuilder, SoftwareRenderingContext,
    WebView, WebViewBuilder, WebViewDelegate,
};

/// A browser engine instance with a single WebView.
pub struct Engine {
    servo: Servo,
    webview: WebView,
    loaded: Rc<Cell<bool>>,
    frames: Rc<Cell<u32>>,
    _wake_rx: Receiver<()>,
    width: u32,
    height: u32,
}

/// A waker driven by a simple thread channel.
#[derive(Clone)]
struct ChannelWaker {
    tx: Sender<()>,
}

impl servo::EventLoopWaker for ChannelWaker {
    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.tx.send(());
    }
}

/// Delegate that drives the compositor. The critical contract is calling
/// `webview.paint()` inside `notify_new_frame_ready`.
struct Delegate {
    loaded: Rc<Cell<bool>>,
    frames: Rc<Cell<u32>>,
}

impl WebViewDelegate for Delegate {
    fn notify_load_status_changed(&self, _webview: WebView, status: LoadStatus) {
        if matches!(status, LoadStatus::Complete) {
            self.loaded.set(true);
        }
    }

    fn notify_new_frame_ready(&self, webview: WebView) {
        // REQUIRED: drives the compositor so the SoftwareRenderingContext's
        // framebuffer actually receives pixels.
        webview.paint();
        self.frames.set(self.frames.get() + 1);
    }
}

impl Engine {
    /// Create a software-rendering engine for a viewport of `width` x `height`
    /// CSS pixels.
    pub fn new(width: u32, height: u32) -> Result<Self, Box<dyn std::error::Error>> {
        // The crypto provider only needs to be installed once; an error here
        // (e.g. already installed) is harmless.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let (wake_tx, wake_rx) = mpsc::channel::<()>();

        // Disable system proxy lookup (a malformed/missing proxy env could
        // stall page loads).
        let mut prefs = Preferences::default();
        prefs.network_http_proxy_uri = String::new();
        prefs.network_https_proxy_uri = String::new();

        let servo = ServoBuilder::default()
            .preferences(prefs)
            .event_loop_waker(Box::new(ChannelWaker { tx: wake_tx }))
            .build();
        servo.setup_logging();

        let size = PhysicalSize::new(width, height);
        let ctx = Rc::new(
            SoftwareRenderingContext::new(size)
                .map_err(|e| format!("SoftwareRenderingContext: {e:?}"))?,
        );
        let _ = RenderingContext::make_current(&*ctx);

        let loaded = Rc::new(Cell::new(false));
        let frames = Rc::new(Cell::new(0u32));
        let delegate = Rc::new(Delegate {
            loaded: loaded.clone(),
            frames: frames.clone(),
        });

        let webview = WebViewBuilder::new(&servo, ctx)
            .url(url::Url::parse("about:blank")?)
            .delegate(delegate)
            .build();
        webview.show();

        Ok(Engine {
            servo,
            webview,
            loaded,
            frames,
            _wake_rx: wake_rx,
            width,
            height,
        })
    }

    /// Load a URL; does not block.
    pub fn load(&self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let parsed = url::Url::parse(url)?;
        self.loaded.set(false);
        self.webview.load(parsed);
        Ok(())
    }

    /// Drive the event loop for up to `timeout`, returning `true` once the page
    /// has reached `LoadStatus::Complete`.
    pub fn wait_for_load(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.servo.spin_event_loop();
            if self.loaded.get() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Capture the current frame as `(width, height, RGBA8 bytes)`.
    ///
    /// Follows Servo's own integration-test pattern exactly:
    ///   1. Wait for `LoadStatus::Complete`.
    ///   2. Force a real repaint with a visible style change (the tests toggle
    ///      `document.body.style.background`), then wait for a new frame.
    ///   3. `read_to_image` (NOT `present`, which wipes the back buffer).
    pub fn snapshot(&self, timeout: Duration) -> Option<(u32, u32, Vec<u8>)> {
        // 1. Wait for load.
        if !self.wait_for_load(timeout) {
            eprintln!("snapshot: timed out waiting for load");
            return None;
        }

        // 2. Force a post-load frame with a visible style-change nudge.
        let js_done = Rc::new(Cell::new(false));
        {
            let js_done = js_done.clone();
            self.webview.evaluate_javascript(
                "requestAnimationFrame(() => { \
                   document.body.style.background = 'red'; \
                   document.body.style.background = ''; \
                 });",
                move |_r| js_done.set(true),
            );
        }
        let deadline = Instant::now() + timeout;
        while !js_done.get() {
            if Instant::now() > deadline {
                eprintln!("snapshot: timed out waiting for JS nudge");
                return None;
            }
            self.servo.spin_event_loop();
            std::thread::sleep(Duration::from_millis(2));
        }

        // 3. Wait for at least one new frame after the nudge.
        let frames_at = self.frames.get();
        while self.frames.get() <= frames_at {
            if Instant::now() > deadline {
                eprintln!("snapshot: timed out waiting for new frame (saw {})", self.frames.get());
                return None;
            }
            self.servo.spin_event_loop();
            std::thread::sleep(Duration::from_millis(2));
        }

        // 4. Read the framebuffer directly.
        let rect = webrender_api::units::DeviceIntRect::from_origin_and_size(
            webrender_api::units::DeviceIntPoint::new(0, 0),
            webrender_api::units::DeviceIntSize::new(self.width as i32, self.height as i32),
        );
        let img = self.webview.rendering_context().read_to_image(rect)?;
        Some((img.width(), img.height(), img.into_raw()))
    }

    /// Current page title, if known.
    pub fn title(&self) -> Option<String> {
        self.webview.page_title()
    }

    /// Current URL, if known.
    pub fn url(&self) -> Option<String> {
        self.webview.url().map(|u| u.to_string())
    }

    pub fn can_go_back(&self) -> bool {
        self.webview.can_go_back()
    }

    pub fn can_go_forward(&self) -> bool {
        self.webview.can_go_forward()
    }

    pub fn go_back(&self) {
        self.loaded.set(false);
        self.webview.go_back(1);
    }

    pub fn go_forward(&self) {
        self.loaded.set(false);
        self.webview.go_forward(1);
    }

    pub fn reload(&self) {
        self.loaded.set(false);
        self.webview.reload();
    }

    /// Extract readable text from the page (DOM `innerText`), used by the
    /// `text` output mode as a functional fallback that needs no pixel
    /// readback.
    pub fn text(&self, timeout: Duration) -> Option<String> {
        let (tx, rx) = mpsc::channel::<Option<String>>();
        // Try innerText first, fall back to textContent (innerText is less
        // complete in some Servo builds; textContent is always available).
        self.webview.evaluate_javascript(
            "document.body ? (document.body.innerText || document.body.textContent) : document.documentElement.textContent",
            move |result| {
                let s = match result {
                    Ok(servo::JSValue::String(s)) => Some(s),
                    Ok(servo::JSValue::Number(n)) => Some(n.to_string()),
                    Ok(servo::JSValue::Boolean(b)) => Some(b.to_string()),
                    _ => None,
                };
                let _ = tx.send(s);
            },
        );
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.servo.spin_event_loop();
            match rx.try_recv() {
                Ok(v) => return v,
                Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(2)),
                Err(mpsc::TryRecvError::Disconnected) => return None,
            }
        }
        None
    }
}