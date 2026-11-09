use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
#[cfg(test)]
mod signature_tests;
fn decode(value: &str) -> Result<String, String> {
    String::from_utf8(STANDARD.decode(value.trim()).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn authenticate(data: &[u8], signature: &str, key: &str) -> Result<String, String> {
    let key = PublicKey::decode(&decode(key)?).map_err(|e| e.to_string())?;
    let signature = Signature::decode(&decode(signature)?).map_err(|e| e.to_string())?;
    // Includes the global signature authenticating the trusted comment. No legacy fallback.
    key.verify(data, &signature, false)
        .map_err(|e| e.to_string())?;
    Ok(signature.trusted_comment().to_owned())
}
fn check_version(comment: &str, version: &str) -> Result<(), String> {
    semver::Version::parse(version).map_err(|e| e.to_string())?;
    let versions: Vec<_> = comment
        .split_whitespace()
        .filter_map(|token| token.strip_prefix("version:"))
        .collect();
    if versions != [version] {
        return Err("Expected exactly one authenticated, exact product version".into());
    }
    Ok(())
}
fn verify(data: &[u8], signature: &str, key: &str, version: &str) -> Result<(), String> {
    let authenticated_comment = authenticate(data, signature, key)?;
    check_version(&authenticated_comment, version)
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 || args[0] != "verify" {
        return Err("Usage: release-policy-tests verify ARTIFACT SIGNATURE VERSION; requires AETHER_UPDATE_PUBLIC_KEY".into());
    }
    let key = std::env::var("AETHER_UPDATE_PUBLIC_KEY")
        .map_err(|_| "Missing AETHER_UPDATE_PUBLIC_KEY")?;
    let data = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let signature = std::fs::read_to_string(&args[2]).map_err(|e| e.to_string())?;
    verify(&data, &signature, &key, &args[3])
}
fn main() {
    if let Err(error) = run() {
        eprintln!("Release signature rejected: {error}");
        std::process::exit(1);
    }
    println!("PASS: updater artifact, public key, trusted comment and version authenticated");
}
