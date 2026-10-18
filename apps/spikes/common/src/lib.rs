//! Shared helpers for the Option C (pure Rust, egui) spikes.
//! See `docs/plans/plan-native-rust-egui.md`, Phase 0.
//!
//! - [`run`] opens an eframe window for a spike.
//! - [`Screenshot`] implements `MARINA_SCREENSHOT=<file.png>` (with
//!   `MARINA_SCREENSHOT_EXIT=1` to close afterwards), as ArtCraft's
//!   `ARTCRAFT_SCREENSHOT` does. On Wayland the compositor stops sending
//!   frame callbacks while the session is locked or the window is hidden,
//!   so nothing is painted and the capture never happens; run with
//!   `WAYLAND_DISPLAY=` (X11/XWayland) or with the session unlocked.
//! - [`Stats`] collects timings and prints percentiles.

use std::path::PathBuf;
use std::time::Duration;

/// Seconds to wait before a `MARINA_SCREENSHOT` capture, so loading work can finish.
const SETTLE_SECS: f64 = 3.0;

/// Opens a window titled `title` running the app `make` builds.
pub fn run<A: eframe::App + 'static>(
    title: &str,
    make: impl FnOnce(&eframe::CreationContext<'_>) -> A + 'static,
) -> eframe::Result {
    init_logging();
    let viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([1200.0, 800.0]);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(title, options, Box::new(|cc| Ok(Box::new(make(cc)))))
}

pub fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}

/// `MARINA_SCREENSHOT=<file.png>` saves a screenshot once the window has settled; call
/// [`Screenshot::tick`] at the end of every frame.
pub struct Screenshot {
    path: PathBuf,
    exit: bool,
    requested: bool,
}

impl Screenshot {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("MARINA_SCREENSHOT")?.into();
        Some(Self {
            path,
            exit: std::env::var_os("MARINA_SCREENSHOT_EXIT").is_some(),
            requested: false,
        })
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        ctx.request_repaint();
        if self.requested || ctx.input(|i| i.time) < SETTLE_SECS {
            return;
        }
        self.requested = true;
        let (path, exit, ctx2) = (self.path.clone(), self.exit, ctx.clone());
        ctx.request_screenshot(move |image| {
            let [w, h] = image.size;
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            match image::RgbaImage::from_raw(w as u32, h as u32, pixels).map(|img| img.save(&path))
            {
                Some(Ok(())) => log::info!("Saved a screenshot to {}", path.display()),
                other => log::error!("Couldn't save the screenshot: {other:?}"),
            }
            if exit {
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx2.request_repaint();
        });
    }
}

/// Timings of one measured operation.
#[derive(Default)]
pub struct Stats {
    samples: Vec<Duration>,
}

impl Stats {
    pub fn push(&mut self, d: Duration) {
        self.samples.push(d);
    }

    /// Times `f` and records it.
    pub fn time<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        let out = f();
        self.push(start.elapsed());
        out
    }

    pub fn percentile(&self, p: f64) -> Duration {
        let mut s = self.samples.clone();
        if s.is_empty() {
            return Duration::ZERO;
        }
        s.sort();
        s[((s.len() - 1) as f64 * p).round() as usize]
    }

    pub fn max(&self) -> Duration {
        self.samples.iter().copied().max().unwrap_or_default()
    }

    /// `label: n=…, p50=…, p95=…, max=…` in milliseconds.
    pub fn summary(&self, label: &str) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        format!(
            "{label}: n={}, p50={:.2} ms, p95={:.2} ms, max={:.2} ms",
            self.samples.len(),
            ms(self.percentile(0.5)),
            ms(self.percentile(0.95)),
            ms(self.max())
        )
    }
}
