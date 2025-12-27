#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let installer = value_for(&args, "--installer")
        .map(PathBuf::from)
        .context("Missing --installer argument")?;
    let app = value_for(&args, "--app")
        .map(PathBuf::from)
        .context("Missing --app argument")?;
    let cleanup = args.iter().any(|arg| arg == "--cleanup");
    let log_path = value_for(&args, "--log").map(PathBuf::from);

    thread::sleep(Duration::from_millis(1500));

    let log_path = log_path.unwrap_or_else(|| installer.with_extension("log"));
    let log_arg = format!("/LOG={}", log_path.display());
    let status = Command::new(&installer)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            &log_arg,
        ])
        .status()
        .with_context(|| format!("Failed to start installer {}", installer.display()))?;

    if !status.success() {
        anyhow::bail!("Installer exited with status {:?}", status.code());
    }

    let _ = Command::new(&app).spawn();

    if cleanup && let Some(parent) = installer.parent() {
        let _ = fs::remove_file(&installer);
        let _ = fs::remove_dir(parent);
    }

    Ok(())
}

fn value_for(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|idx| args.get(idx + 1))
        .cloned()
}
