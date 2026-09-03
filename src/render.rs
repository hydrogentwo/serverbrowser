//! Servo rendering: navigate to a URL and produce RGBA pixels using Servo's
//! software (headless) rendering path, then hand them to the output layer.
//!
//! This is the bridge between the browser engine and the terminal. It uses
//! [`servo::SoftwareRenderingContext`] so no window system or GPU is required.

use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use servo::{Servo, ServoBuilder, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate};
use dpi::PhysicalSize;

/// A browser engine instance with a single WebView.
///
/// Servo is event-driven: you `load` a URL, then repeatedly call
/// [`Self::spin`] until a new frame is ready.
pub struct Engine {
    servo: Servo,
    webview: WebView,
    frame_ready: Rc<std::cell::Cell<bool>>,
    _wake_rx: Receiver<()>,
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

/// Tracks frame readiness via `notify_new_frame_ready`.
struct Delegate {
    frame_ready: Rc<std::cell::Cell<bool>>,
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        self.frame_ready.set(true);
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

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(ChannelWaker { tx: wake_tx }))
            .build();

        let size = PhysicalSize::new(width, height);
        let ctx = Rc::new(
            SoftwareRenderingContext::new(size)
                .map_err(|e| format!("SoftwareRenderingContext: {e:?}"))?,
        );

        let frame_ready = Rc::new(std::cell::Cell::new(false));
        let delegate = Rc::new(Delegate {
            frame_ready: frame_ready.clone(),
        });

        let webview = WebViewBuilder::new(&servo, ctx)
            .url(url::Url::parse("about:blank")?)
            .delegate(delegate)
            .build();

        Ok(Engine {
            servo,
            webview,
            frame_ready,
            _wake_rx: wake_rx,
        })
    }

    /// Load a URL; does not block.
    pub fn load(&self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let parsed = url::Url::parse(url)?;
        self.frame_ready.set(false);
        self.webview.load(parsed);
        Ok(())
    }

    /// Spin the event loop once.
    fn spin(&self) {
        self.servo.spin_event_loop();
    }

    /// Drive the event loop for up to `timeout`, returning `true` once a new
    /// frame is ready.
    pub fn wait_for_frame(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.spin();
            if self.frame_ready.get() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Capture the current frame as RGBA8 bytes (width*height*4). If no frame
    /// has been rendered yet, returns `None`.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        self.webview.take_screenshot(None, move |res| {
            let _ = tx.send(res);
        });
        // Drive the loop so the async screenshot request completes.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            self.spin();
            match rx.try_recv() {
                Ok(Ok(img)) => return Some(img.into_raw()),
                Ok(Err(_)) => return None,
                Err(mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(mpsc::TryRecvError::Disconnected) => return None,
            }
        }
        None
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
        self.frame_ready.set(false);
        self.webview.go_back(1);
    }

    pub fn go_forward(&self) {
        self.frame_ready.set(false);
        self.webview.go_forward(1);
    }

    pub fn reload(&self) {
        self.frame_ready.set(false);
        self.webview.reload();
    }
}