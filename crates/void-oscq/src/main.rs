//! Void OSCQ desktop application. UI only, all protocol work lives in `oscq-core` and `oscq-discovery`.

// A window application: no console window on Windows. Debug builds keep it for the logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod json_view;
mod rate;
mod state;
mod traffic;
mod tree;
mod widget;

/// The synthetic OSCQuery server of `oscq-core`'s tests, shared by the budget measurements.
#[cfg(test)]
#[path = "../../oscq-core/tests/common/mod.rs"]
mod synthetic;

use anyhow::{Context as _, anyhow};

fn main() -> anyhow::Result<()> {
    let launched = std::time::Instant::now();
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
        .with_inner_size([1100.0, 700.0]);
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
