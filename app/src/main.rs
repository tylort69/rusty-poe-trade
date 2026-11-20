//! Rusty POE Trade — a Path of Exile price-check & trading overlay.
//! A Rust rewrite of Awakened PoE Trade (MIT) with a Material You UI.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod fonts;
mod host;
mod i18n;
mod platform_glue;
mod services;
mod store;
mod widgets;
mod wm;

fn main() -> eframe::Result {
    // The game runs under Proton/Wine as an XWayland client, so the overlay must be
    // an X11 window too (Wayland forbids global positioning/always-on-top/input
    // passthrough for other apps' windows). Must happen before any threads start.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("RPT_ALLOW_WAYLAND").is_none() {
        // SAFETY: single-threaded at this point in `main`.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn".into()))
        .init();

    let windowed = std::env::args().any(|a| a == "--windowed");
    let store = store::ConfigStore::new();
    let cfg = store.load();
    if let Err(e) = rpt_data::init(&cfg.language) {
        tracing::error!("failed to load game data for {}: {e}; falling back to en", cfg.language);
        rpt_data::init("en").expect("embedded en data");
    }

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Rusty POE Trade")
        .with_app_id("rusty-poe-trade");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    viewport = if windowed {
        viewport.with_inner_size([1280.0, 800.0]).with_min_inner_size([800.0, 600.0])
    } else {
        viewport
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_mouse_passthrough(true)
            .with_position([0.0, 0.0])
            .with_inner_size([1280.0, 720.0])
    };

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "rusty-poe-trade",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, store, cfg, app::LaunchOptions { windowed })))),
    )
}
