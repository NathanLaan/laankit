//! The system file dialogs, without freezing the window.
//!
//! `rfd::FileDialog` blocks until the user answers. On Linux it talks to the
//! desktop portal, and while it waits the window stops answering the
//! compositor, which then offers to close the "not responding" app. So there
//! the dialog runs on its own thread and the answer arrives on a later frame.
//! On Windows and macOS the dialog is modal and keeps the window's event loop
//! running itself (macOS also wants it on the main thread), so it runs in
//! place, as do the tests' stand-ins.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

/// One dialog at a time; `T` says what the answer is for.
pub struct Picker<T> {
    tx: Sender<(T, Option<PathBuf>)>,
    rx: Receiver<(T, Option<PathBuf>)>,
    busy: bool,
    ctx: Option<egui::Context>,
}

impl<T: Send + 'static> Picker<T> {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            busy: false,
            ctx: None,
        }
    }

    /// The context to wake when an answer arrives.
    pub fn attach(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
    }

    /// Whether a dialog is open.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Shows a dialog (`ask`), unless one is open already.
    pub fn start(&mut self, what: T, ask: impl FnOnce() -> Option<PathBuf> + Send + 'static) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let answer = move || {
            let _ = tx.send((what, ask()));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        if cfg!(all(unix, not(target_os = "macos"), not(test))) {
            std::thread::spawn(answer);
        } else {
            answer();
        }
    }

    /// The picked path, once the dialog has closed with one.
    pub fn poll(&mut self) -> Option<(T, PathBuf)> {
        let (what, path) = self.rx.try_recv().ok()?;
        self.busy = false;
        Some((what, path?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_dialog_at_a_time_and_cancelling_frees_it() {
        let mut p = Picker::new();
        p.start(1, || None);
        assert!(p.busy());
        assert_eq!(p.poll(), None, "cancelled");
        assert!(!p.busy());

        p.start(2, || Some(PathBuf::from("a")));
        p.start(3, || Some(PathBuf::from("b")));
        assert_eq!(p.poll(), Some((2, PathBuf::from("a"))));
        assert_eq!(p.poll(), None, "the second was refused");
    }
}
