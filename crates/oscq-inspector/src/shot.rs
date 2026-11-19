//! Writing a screenshot of the window from inside the application, for the documentation images
//! and nothing else. Compiled only with the `screenshot` feature, which no release build takes:
//! the shipped binary carries none of this code, so the size budget of PRD 10 is untouched.
//!
//! Why the application draws its own: the images of `docs/images/screenshots` were taken with a
//! window capture of the system's own tool, and on this machine that capture is refused while a
//! capture of the whole screen is allowed. Three of those images also caught a window that sat
//! behind another, where macOS composites nothing and the frame never changes, and they show the
//! state of the first two seconds rather than the view asked for (`docs/images/screenshots/
//! README.md`). A frame the application reads back from its own renderer has neither problem: it
//! is what was drawn, whatever covers the window and whatever desktop it sits on.
//!
//! ```text
//! OSCQ_INSPECTOR_SHOT=/path/to/out.png OSCQ_INSPECTOR_SHOT_AFTER_MS=2500 \
//!   OSCQ_INSPECTOR_SHOT_DO=capture:3 oscq-inspector
//! ```
//!
//! `OSCQ_INSPECTOR_SHOT_DO` carries the words of
//! [`crate::app::InspectorApp::scripted`], performed once the window has settled and before the
//! frame is read back: the actions of a hand that is not there. One word per frame, with a pause
//! between, because a hand clicks once per frame: two words in one frame would have the window
//! write its config twice at once, which it does not survive (`PITFALLS.md`).
//!
//! The window closes once the file is written, so one image is one command, as the startup words
//! of [`crate::app::InspectorApp::startup`] intend.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Between two words of the script: long enough for the window to have drawn, written its config
/// and heard the server, as it would between two clicks.
const WORD_PAUSE: Duration = Duration::from_millis(600);

/// What the environment asks of this launch, and where the sequence has got to.
pub struct Shot {
    path: PathBuf,
    /// The words of `OSCQ_INSPECTOR_SHOT_DO` left to perform, one per frame, in order.
    script: Vec<String>,
    /// How long the window draws after the script before the frame is read back.
    settle: Duration,
    state: State,
}

enum State {
    /// Drawing frames until `until`, so that the servers have answered and the view has settled.
    /// Then the next word of the script is performed, alone in its frame.
    Settling { until: Instant },
    /// Every word has been performed; the window draws until `until` so that what the last one
    /// asked for is on screen before the frame is read back.
    Settled { until: Instant },
    /// The screenshot is asked for; the renderer answers with the frame it drew.
    Asked,
    /// Written, or refused: nothing more is asked of the window.
    Done,
}

impl Shot {
    /// The shot this launch asks for, or `None`. `OSCQ_INSPECTOR_SHOT` is the file to write;
    /// `OSCQ_INSPECTOR_SHOT_AFTER_MS` is how long the window draws first, 2500 ms by default.
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("OSCQ_INSPECTOR_SHOT")?;
        let after = std::env::var("OSCQ_INSPECTOR_SHOT_AFTER_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2500);
        let settle = Duration::from_millis(after);
        Some(Self {
            path: PathBuf::from(path),
            script: std::env::var("OSCQ_INSPECTOR_SHOT_DO")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect(),
            settle,
            state: State::Settling {
                until: Instant::now() + settle,
            },
        })
    }

    /// One frame of the sequence, called at the end of every frame.
    pub fn tick(&mut self, app: &mut crate::app::InspectorApp, ctx: &egui::Context) {
        match self.state {
            State::Settling { until } => {
                let now = Instant::now();
                if now < until {
                    // The window is asked to keep drawing: at rest it would not, and the frame
                    // read back would be whatever the first frames left.
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
                // One word, alone in its frame, then a pause: what it asked for needs frames
                // of its own, a capture writing the config and a recall waiting for the server.
                if self.script.is_empty() {
                    ctx.request_repaint();
                    self.state = State::Settled {
                        until: now + self.settle,
                    };
                    return;
                }
                let word = self.script.remove(0);
                app.scripted(ctx, &word);
                ctx.request_repaint();
                self.state = State::Settling {
                    until: now + WORD_PAUSE,
                };
            }
            State::Settled { until } => {
                if Instant::now() < until {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                ctx.request_repaint();
                self.state = State::Asked;
            }
            State::Asked => {
                let image = ctx.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Screenshot { image, .. } => Some(image.clone()),
                        _ => None,
                    })
                });
                let Some(image) = image else {
                    ctx.request_repaint();
                    return;
                };
                match write_png(&self.path, &image) {
                    Ok(()) => tracing::info!(
                        target: "oscq_inspector::shot",
                        "{} by {} written to {}",
                        image.size[0],
                        image.size[1],
                        self.path.display()
                    ),
                    Err(e) => tracing::error!(
                        target: "oscq_inspector::shot",
                        "{} not written: {e}",
                        self.path.display()
                    ),
                }
                self.state = State::Done;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            State::Done => {}
        }
    }
}

fn write_png(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (w, h) = (image.size[0] as u32, image.size[1] as u32);
    let mut bytes = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for p in &image.pixels {
        let [r, g, b, a] = p.to_srgba_unmultiplied();
        bytes.extend_from_slice(&[r, g, b, a]);
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder
        .write_header()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .write_image_data(&bytes)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}
