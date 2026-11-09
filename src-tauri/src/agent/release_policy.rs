#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_insecure_credentialed_and_local_urls() {
        for value in [
            "http://updates.example.com/latest.json",
            "https://user:secret@updates.example.com/latest.json",
            "https://localhost/u",
            "https://127.0.0.1/u",
            "https://[::1]/u",
            "https://10.0.0.2/u",
            "https://updates.example.com/u?token=secret",
            "https://updates.example.com/u#secret",
        ] {
            assert!(validate_url(value).is_err(), "accepted {value}");
        }
        assert!(validate_url("https://updates.example.com/latest.json").is_ok());
    }
}
/// Release infrastructure must be public HTTPS with no embedded credentials.
/// Literal IPs are deliberately excluded, including IPv4-mapped IPv6 addresses.
pub fn validate_url(value: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(value).map_err(|_| "Invalid update URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Update URLs require HTTPS without credentials, queries or fragments".into());
    }
    match url.host() {
        Some(url::Host::Domain(host))
            if host.contains('.')
                && !host.ends_with('.')
                && ![".localhost", ".local", ".internal", ".test", ".invalid"]
                    .iter()
                    .any(|suffix| host.ends_with(suffix)) =>
        {
            Ok(url)
        }
        _ => Err("Update URLs require a public DNS hostname".into()),
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;
    #[test]
    fn requires_strictly_newer_semver() {
        assert!(is_newer("0.3.0", "0.3.1"));
        for candidate in ["0.2.9", "0.3.0", "0.3.0+newbuild", "invalid"] {
            assert!(!is_newer("0.3.0", candidate));
        }
    }
    #[test]
    fn rejects_unusable_public_keys_and_missing_configuration() {
        assert!(validate_key("not a key").is_err());
        assert!(validate_key("").is_err());
        use base64::Engine;
        let packet = [b"Ed".as_slice(), &[1u8; 40]].concat();
        let inner = base64::engine::general_purpose::STANDARD.encode(packet);
        let valid = base64::engine::general_purpose::STANDARD
            .encode(format!("untrusted comment: test key\n{inner}\n"));
        assert!(validate_key(&valid).is_ok());
    }
}
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(candidate),
    ) {
        (Ok(current), Ok(candidate)) => candidate.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}
/// Tauri accepts a base64-encoded minisign public-key file (Ed25519).
pub fn validate_key(key: &str) -> Result<(), String> {
    use base64::Engine;
    let decode = |value: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| "Invalid updater public key".to_string())
    };
    let text = String::from_utf8(decode(key.trim())?).map_err(|_| "Invalid updater public key")?;
    let packet = decode(
        text.lines()
            .find(|line| !line.starts_with("untrusted comment:") && !line.trim().is_empty())
            .ok_or("Missing updater public key")?
            .trim(),
    )?;
    if packet.len() != 42 || &packet[..2] != b"Ed" || packet[10..].iter().all(|byte| *byte == 0) {
        return Err("Expected an Ed25519 minisign public key".into());
    }
    Ok(())
}
