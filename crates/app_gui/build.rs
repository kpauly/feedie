use std::{env, path::PathBuf};

fn main() {
    // Rebuild when version or Roboflow key changes so baked-in envs stay in sync.
    println!("cargo:rerun-if-env-changed=FEEDIE_VERSION");
    println!("cargo:rerun-if-env-changed=FEEDIE_ROBOFLOW_API_KEY");

    let version =
        env::var("FEEDIE_VERSION").unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=FEEDIE_VERSION={version}");

    let roboflow = env::var("FEEDIE_ROBOFLOW_API_KEY").unwrap_or_default();
    println!("cargo:rustc-env=ROBOFLOW_API_KEY={roboflow}");

    #[cfg(target_os = "windows")]
    apply_windows_icon();
}

#[cfg(target_os = "windows")]
fn apply_windows_icon() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon_path = manifest_dir
        .join("..")
        .join("..")
        .join("assets")
        .join("Feedie.ico");
    println!("cargo:rerun-if-changed={}", icon_path.display());
    if !icon_path.exists() {
        println!(
            "cargo:warning=Windows icon missing at {}",
            icon_path.display()
        );
        return;
    }
    if let Some(icon_str) = icon_path.to_str() {
        let mut res = winres::WindowsResource::new();
        res.set_icon(icon_str);
        if let Err(err) = res.compile() {
            println!("cargo:warning=Failed to embed Windows icon: {err}");
        }
    } else {
        println!("cargo:warning=Windows icon path is not valid UTF-8");
    }
}
