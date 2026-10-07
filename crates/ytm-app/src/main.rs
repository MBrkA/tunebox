#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod backend;
mod history;
mod i18n;
mod layout;
mod local;
mod media;
mod notify;
mod session;
mod single;
mod state;
mod theme;
mod thumbs;
mod tray;
mod views;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use eframe::egui;
use tokio::sync::mpsc;
use ytm_api::{ChainResolver, InnerTube, MusicApi};
use ytm_player::{Player, PlayerOptions};

use crate::app::{DevOptions, TuneboxApp};
use crate::backend::Backend;
use crate::state::AppState;
use crate::thumbs::{ThumbBytesLoader, ThumbLoader};

fn parse_dev_options() -> DevOptions {
    let mut dev = DevOptions {
        shot_delay: 4.0,
        ..DevOptions::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => dev.screenshot = args.next().map(PathBuf::from),
            "--query" => dev.query = args.next(),
            "--play-first" => dev.play_first = true,
            "--now-playing" => dev.open_now_playing = true,
            "--queue" => dev.open_queue = true,
            "--help-overlay" => dev.open_help = true,
            "--size" => {
                dev.size = args.next().and_then(|v| {
                    let (w, h) = v.split_once('x')?;
                    Some([w.parse().ok()?, h.parse().ok()?])
                });
            }
            "--filter" => dev.filter = args.next(),
            "--route" => dev.route = args.next(),
            "--local-demo" => dev.local_demo = true,
            "--edit-playlist" => dev.edit_playlist = true,
            "--new-playlist" => dev.new_playlist_dialog = true,
            "--open-menu" => DEV_OPEN_MENU.store(true, std::sync::atomic::Ordering::Relaxed),
            "--shot-delay" => {
                dev.shot_delay = args.next().and_then(|v| v.parse().ok()).unwrap_or(4.0);
            }
            other => tracing::warn!(arg = other, "unknown argument"),
        }
    }
    dev
}

/// The Win32 window handle, which the Windows media-transport controls need (0 elsewhere).
#[cfg(windows)]
fn native_window_handle(cc: &eframe::CreationContext<'_>) -> Option<usize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as usize),
        _ => None,
    }
}

#[cfg(not(windows))]
fn native_window_handle(_cc: &eframe::CreationContext<'_>) -> Option<usize> {
    None
}

fn launch(
    renderer: eframe::Renderer,
    rt: &tokio::runtime::Runtime,
    config: &ytm_core::Config,
    dev: &DevOptions,
) -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_icon(
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/256x256.png"))
                .expect("bundled icon is a valid PNG"),
        )
        .with_decorations(!views::titlebar::enabled())
        .with_title("Tunebox")
        .with_app_id("tunebox")
        .with_inner_size(dev.size.unwrap_or([1280.0, 820.0]))
        .with_min_inner_size(layout::MIN_WINDOW);
    let options = eframe::NativeOptions {
        viewport,
        renderer,
        ..Default::default()
    };
    i18n::set(i18n::resolve(&config.ui_language));
    let theme_pref = theme::Pref::from_code(&config.theme);
    let handle = rt.handle().clone();
    let (mut config, dev) = (config.clone(), dev.clone());
    // YouTube's own text follows the interface language.
    config.language = i18n::resolve(&config.ui_language).hl().into();
    eframe::run_native(
        "Tunebox",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            single::set_context(ctx.clone());
            theme::install(&ctx);
            theme::apply_pref(&ctx, theme_pref);
            egui_extras::install_image_loaders(&ctx);

            let cache_dir = ytm_core::Config::dirs()
                .map(|d| d.cache_dir().join("thumbnails"))
                .unwrap_or_else(|_| std::env::temp_dir().join("tunebox-thumbs"));
            let thumbs = ThumbLoader::new(handle.clone(), cache_dir, config.thumbnail_cache_mb);
            ctx.add_bytes_loader(Arc::new(ThumbBytesLoader(thumbs.clone())));

            let tube = Arc::new(InnerTube::from_config(&config)?);
            let resolver = Arc::new(ChainResolver::standard(tube.clone(), &config));
            let repaint = ctx.clone();
            let player = Player::spawn(
                &handle,
                resolver,
                PlayerOptions {
                    volume: config.volume,
                    notify: Some(Arc::new(move || repaint.request_repaint())),
                    ..PlayerOptions::default()
                },
            )?;

            if config.restore_session {
                if let Some(cmd) = session::load().and_then(session::Session::into_command) {
                    player.send(cmd);
                }
            }

            #[cfg(target_os = "macos")]
            {
                notify::set_enabled(config.notifications);
                notify::spawn_watcher(&handle, player.clone());
            }
            let (action_tx, action_rx) = mpsc::unbounded_channel();
            media::spawn(
                &handle,
                action_tx.clone(),
                player.clone(),
                native_window_handle(cc),
            );
            let tray = tray::Tray::start(&handle, ctx.clone(), action_tx.clone(), config.tray_icon);
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            let api: Arc<dyn MusicApi> = tube.clone();
            let local_path = local::LocalLibrary::default_path();
            Backend::spawn(
                &handle,
                api,
                local_path.clone(),
                player.clone(),
                ctx,
                action_rx,
                event_tx,
            );

            let (library, warning) = match &local_path {
                Some(p) => local::LocalLibrary::load(p),
                None => (local::LocalLibrary::default(), None),
            };
            let history = match &local_path {
                Some(p) => history::History::load(&history::file_for(p)),
                None => history::History::default(),
            };
            let mut state = AppState::new(player, action_tx)
                .with_config(config.clone())
                .with_local(library)
                .with_history(history);
            if let Some(w) = warning {
                state.toast(w);
            }
            let pending = state.local.pending_ids();
            if !pending.is_empty() {
                state.send(backend::Action::LoadPlaylistTracks(pending));
            }
            Ok(Box::new(TuneboxApp::new(
                state, event_rx, thumbs, dev, tray,
            )))
        }),
    )
}

/// Dev flag `--open-menu`: keep the playlist's Edit dropdown open (for screenshots).
pub static DEV_OPEN_MENU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Process start, for the cold-start measurement logged on the first frame.
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Wayland cannot hide a window or unminimize it from the client, and a minimized window gets no
/// frame callbacks, so the tray's Show / Quit would never be processed. With "close to tray" on we
/// therefore run through XWayland, where hide/show and wake-ups work (D28).
fn prefer_x11_for_tray(config: &ytm_core::Config) {
    if cfg!(target_os = "linux")
        && config.tray_icon
        && config.close_to_tray
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("DISPLAY").is_some()
    {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}

fn main() -> Result<()> {
    START.get_or_init(std::time::Instant::now);
    ytm_core::logging::init();
    if matches!(single::acquire(), single::Instance::Existing) {
        return Ok(());
    }
    let config = ytm_core::Config::load().context("loading config")?;
    prefer_x11_for_tray(&config);
    let dev = parse_dev_options();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("ytm-rt")
        .build()
        .context("starting async runtime")?;
    let _guard = rt.enter();

    // wgpu is preferred; fall back to glow when no suitable GPU/driver is found.
    if let Err(e) = launch(eframe::Renderer::Wgpu, &rt, &config, &dev) {
        tracing::warn!(error = %e, "wgpu backend failed, falling back to glow");
        launch(eframe::Renderer::Glow, &rt, &config, &dev)
            .map_err(|e| anyhow::anyhow!("could not start UI: {e}"))?;
    }
    Ok(())
}
