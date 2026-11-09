#[allow(dead_code)]
#[path = "src/agent/release_policy.rs"]
mod release_policy;
use std::process::Command;
fn git(args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
fn main() {
    // Only explicitly allowlisted public updater values are embedded; never copy process env.
    for name in ["AETHER_UPDATE_ENDPOINT", "AETHER_UPDATE_PUBLIC_KEY"] {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var(name).unwrap_or_default();
        assert!(
            !value.chars().any(char::is_control),
            "Updater configuration must be a single line"
        );
        if !value.is_empty() {
            if name == "AETHER_UPDATE_ENDPOINT" {
                release_policy::validate_url(&value).expect("Unsafe updater endpoint");
            } else {
                release_policy::validate_key(&value).expect("Invalid Ed25519 updater public key");
            }
        }
        println!("cargo:rustc-env={name}={value}");
    }
    let sha = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = match git(&["status", "--porcelain"]) {
        Some(status) if status.is_empty() => "clean",
        Some(_) => "dirty",
        None => "unknown",
    };
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string();
    println!("cargo:rustc-env=AETHER_BUILD_ID={stamp}-{sha}-{dirty}");
    // Re-run for source changes, not just build.rs, to avoid a stale dirty marker.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=../src");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
    println!("cargo:rerun-if-changed=../.git/refs");
    tauri_build::build()
}
