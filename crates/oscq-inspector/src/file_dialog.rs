//! The native open and save dialogs of the File menu (PRD 9.11), behind a function the tests
//! replace.
//!
//! The dialog is asked for on the UI thread and answered on the runtime: asking only starts it
//! (a sheet on the window on macOS, a dialog of its own thread on Windows, the desktop portal on
//! Linux) and returns a future, which the runtime awaits. The UI thread never waits on it
//! (CLAUDE.md law 6).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

/// What a dialog is asked.
#[derive(Debug, Clone)]
pub struct Ask {
    /// A save dialog; an open dialog otherwise.
    pub save: bool,
    pub title: &'static str,
    /// The name a save dialog proposes.
    pub file_name: String,
    /// The kind of file shown, and its extensions.
    pub filter: (&'static str, &'static [&'static str]),
    /// Where the dialog opens.
    pub directory: Option<PathBuf>,
}

/// The file chosen, or nothing when the dialog was cancelled.
pub type Answer = Pin<Box<dyn Future<Output = Option<PathBuf>> + Send>>;

#[derive(Clone)]
pub struct Picker(Arc<dyn Fn(Ask) -> Answer + Send + Sync>);

impl Picker {
    /// The system's own dialogs.
    pub fn native() -> Self {
        Self(Arc::new(|ask: Ask| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .set_title(ask.title)
                .add_filter(ask.filter.0, ask.filter.1);
            if let Some(dir) = ask.directory {
                dialog = dialog.set_directory(dir);
            }
            if ask.save {
                let chosen = dialog.set_file_name(ask.file_name).save_file();
                Box::pin(async move { chosen.await.map(|f| f.path().to_path_buf()) })
            } else {
                let chosen = dialog.pick_file();
                Box::pin(async move { chosen.await.map(|f| f.path().to_path_buf()) })
            }
        }))
    }

    /// No dialog: every one is cancelled. The tests' default, so that none opens a window.
    pub fn none() -> Self {
        Self::new(|_| Box::pin(async { None }))
    }

    pub fn new(f: impl Fn(Ask) -> Answer + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    pub fn ask(&self, ask: Ask) -> Answer {
        (self.0)(ask)
    }
}
