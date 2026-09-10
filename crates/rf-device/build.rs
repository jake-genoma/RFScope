use std::{env, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if env::var_os("CARGO_FEATURE_HACKRF").is_none() {
        return;
    }
    let output = Command::new("pkg-config")
        .args(["--libs", "libhackrf"])
        .output()
        .expect("HackRF feature requires pkg-config and Fedora hackrf-devel");
    assert!(
        output.status.success(),
        "libhackrf development files missing: install Fedora hackrf-devel: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for flag in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else if let Some(lib) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={lib}");
        } else {
            println!("cargo:rustc-link-arg={flag}");
        }
    }
}
