//! Void OSCQ desktop application. UI only, all protocol work lives in `oscq-core` and `oscq-discovery`.

// A window application: no console window on Windows. Debug builds keep it for the logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod icon;
mod json_view;
mod presets;
mod rate;
mod state;
mod style;
mod traffic;
mod tree;
mod widget;
#[cfg(test)]
mod winres;

/// The synthetic OSCQuery server of `oscq-core`'s tests, shared by the budget measurements.
#[cfg(test)]
#[path = "../../oscq-core/tests/common/mod.rs"]
mod synthetic;

use anyhow::{Context as _, anyhow};

fn main() -> anyhow::Result<()> {
    let launched = std::time::Instant::now();
    // Before any thread exists, or the system will not tell the local time offset.
    app::capture_local_offset();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Read once at startup, before the UI loop exists; every later write goes through the runtime.
    let dir = config::app_dir();
    let (config, warning) = match &dir {
        Some(d) => config::Config::load(&config::config_path(d)),
        None => (
            config::Config::default(),
            Some("no config directory: settings will not be saved".to_owned()),
        ),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("oscq-net")
        .build()
        .context("starting the network runtime")?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Void OSCQ")
        .with_min_inner_size([640.0, 400.0])
        .with_inner_size([1100.0, 700.0])
        .with_icon(window_icon());
    if let Some(w) = config.window {
        viewport = viewport
            .with_inner_size([w.width, w.height])
            .with_position([w.x, w.y]);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Void OSCQ",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::VoidApp::start(
                launched,
                cc.egui_ctx.clone(),
                runtime,
                config,
                warning,
                dir,
            )))
        }),
    )
    .map_err(|e| anyhow!("window: {e}"))
}

/// The icon of the window, of the taskbar, and of the Dock while running: given, or eframe shows
/// its own. On macOS it matches the bundle's, drawn on the macOS grid.
fn window_icon() -> egui::IconData {
    const SIZE: u32 = 256;
    let layout = if cfg!(target_os = "macos") {
        icon::Layout::MacOs
    } else {
        icon::Layout::Full
    };
    egui::IconData {
        rgba: icon::rgba(SIZE, layout),
        width: SIZE,
        height: SIZE,
    }
}
