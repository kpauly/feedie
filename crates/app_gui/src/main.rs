#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Entry point for the Feedie egui desktop application.

mod app;
mod export;
mod i18n;
mod manifest;
mod model;
mod roboflow;
mod settings_store;
mod util;

use app::UiApp;
use eframe::{NativeOptions, egui};
use egui::viewport::ViewportBuilder;
use std::sync::Arc;
use util::load_app_icon;

#[cfg(target_os = "linux")]
use std::{env, fs, path::Path};

#[cfg(target_os = "linux")]
fn is_crostini() -> bool {
    env::var_os("CROS_USER_ID_HASH").is_some()
        || env::var_os("SOMMELIER_VERSION").is_some()
        || env::var_os("SOMMELIER_SCALE").is_some()
        || Path::new("/dev/.cros_milestone").exists()
        || Path::new("/mnt/chromeos").is_dir()
        || fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|value| {
                let value = value.to_lowercase();
                value.contains("cros") || value.contains("termina")
            })
            .unwrap_or(false)
        || fs::read_to_string("/proc/version")
            .map(|value| {
                let value = value.to_lowercase();
                value.contains("chromeos") || value.contains("chromium") || value.contains("cros")
            })
            .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn apply_crostini_x11_workaround() {
    if !is_crostini() {
        return;
    }
    if env::var_os("WINIT_UNIX_BACKEND").is_none() {
        // SAFETY: set before any threads spawn or libraries read env vars.
        unsafe {
            env::set_var("WINIT_UNIX_BACKEND", "x11");
        }
    }
    if env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: set before any threads spawn or libraries read env vars.
        unsafe {
            env::set_var("GDK_BACKEND", "x11");
        }
    }
}

/// Bootstraps the egui application and installs tracing and the window icon.
fn main() {
    #[cfg(debug_assertions)]
    tracing_subscriber::fmt::init();

    #[cfg(target_os = "linux")]
    apply_crostini_x11_workaround();

    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_icon(Arc::new(load_app_icon())),
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "Feedie",
        options,
        Box::new(|_cc| {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Box::new(UiApp::default()))
        }),
    ) {
        eprintln!("Applicatie gestopt met fout: {err}");
    }
}
