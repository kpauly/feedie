use std::{env, fs, path::PathBuf};

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
    copy_openmp_runtime();
}

#[cfg(target_os = "windows")]
fn copy_openmp_runtime() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let runtime = manifest_dir
        .join("..")
        .join("..")
        .join("runtime")
        .join("windows")
        .join("libiomp5md.dll");
    println!("cargo:rerun-if-changed={}", runtime.display());
    if !runtime.exists() {
        println!(
            "cargo:warning=Missing Intel OpenMP runtime at {}",
            runtime.display()
        );
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = match out_dir.ancestors().nth(3) {
        Some(dir) => dir.to_path_buf(),
        None => {
            println!("cargo:warning=Unable to locate target directory from OUT_DIR");
            return;
        }
    };

    let dest = target_dir.join("libiomp5md.dll");
    if let Err(err) = fs::copy(&runtime, &dest) {
        println!(
            "cargo:warning=Failed to copy OpenMP runtime to {}: {}",
            dest.display(),
            err
        );
    }

    let deps_dir = target_dir.join("deps");
    if let Err(err) = fs::create_dir_all(&deps_dir) {
        println!(
            "cargo:warning=Failed to create deps dir {}: {}",
            deps_dir.display(),
            err
        );
        return;
    }
    let deps_dest = deps_dir.join("libiomp5md.dll");
    if let Err(err) = fs::copy(&runtime, &deps_dest) {
        println!(
            "cargo:warning=Failed to copy OpenMP runtime to {}: {}",
            deps_dest.display(),
            err
        );
    }
}
