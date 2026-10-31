//! The system file dialogs (`rfd`), without freezing the window.
//!
//! `rfd::FileDialog` blocks until the user answers. On Linux it talks to the
//! desktop portal, and while it waits the window stops answering the
//! compositor, which then offers to close the "not responding" app. So there
//! [`FileDialogs`] runs the dialog on its own thread and the answer arrives
//! on a later frame. On Windows and macOS the dialog is modal and keeps the
//! window's event loop running itself (macOS also wants it on the main
//! thread), so it runs in place.
//!
//! ```ignore
//! enum Picked { Folder(PathBuf) }
//! // Starting it:
//! self.dialogs.start(|| rfd::FileDialog::new().pick_folder().map(Picked::Folder));
//! // Every frame:
//! if let Some(Picked::Folder(folder)) = self.dialogs.poll() { … }
//! ```

use std::sync::mpsc::{channel, Receiver, Sender};

/// One file dialog at a time. `T` is the answer: what was picked and what
/// it's for. A cancelled dialog answers nothing.
pub struct FileDialogs<T> {
    tx: Sender<Option<T>>,
    rx: Receiver<Option<T>>,
    busy: bool,
    threaded: bool,
    ctx: Option<egui::Context>,
}

impl<T: Send + 'static> Default for FileDialogs<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> FileDialogs<T> {
    /// Dialogs on their own thread on Linux and the BSDs, in place elsewhere.
    pub fn new() -> Self {
        Self::with_thread(cfg!(all(unix, not(target_os = "macos"))))
    }

    /// Always in place: the answer is there on the next [`Self::poll`]. For
    /// tests, whose stand-in "dialogs" answer at once.
    pub fn in_place() -> Self {
        Self::with_thread(false)
    }

    fn with_thread(threaded: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            busy: false,
            threaded,
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
    pub fn start(&mut self, ask: impl FnOnce() -> Option<T> + Send + 'static) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let answer = move || {
            let _ = tx.send(ask());
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        if self.threaded {
            std::thread::spawn(answer);
        } else {
            answer();
        }
    }

    /// The answer, once the dialog has closed with one.
    pub fn poll(&mut self) -> Option<T> {
        let answer = self.rx.try_recv().ok()?;
        self.busy = false;
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_dialog_at_a_time_and_cancelling_frees_it() {
        let mut d = FileDialogs::in_place();
        d.start(|| None);
        assert!(d.busy());
        assert_eq!(d.poll(), None, "cancelled");
        assert!(!d.busy());

        d.start(|| Some(1));
        d.start(|| Some(2));
        assert_eq!(d.poll(), Some(1));
        assert_eq!(d.poll(), None, "the second was refused");
    }

    #[test]
    fn a_threaded_answer_arrives_later() {
        let mut d = FileDialogs::with_thread(true);
        d.start(|| Some("picked"));
        let mut answer = None;
        for _ in 0..200 {
            answer = d.poll();
            if answer.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(answer, Some("picked"));
        assert!(!d.busy());
    }
}
