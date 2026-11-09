use base64::{engine::general_purpose::STANDARD, Engine};
const KEY: &str =
    "untrusted comment: fixture\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";
const SIG: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
#[test]
fn authenticates_artifact_and_comment_before_version_policy() {
    let key = STANDARD.encode(KEY);
    let sig = STANDARD.encode(SIG);
    assert!(super::authenticate(b"test", &sig, &key).is_ok());
    assert!(super::authenticate(b"Test", &sig, &key).is_err());
    let mut lines: Vec<_> = SIG.lines().map(str::to_owned).collect();
    let mut packet = STANDARD.decode(&lines[1]).unwrap();
    packet[10] ^= 1;
    lines[1] = STANDARD.encode(packet);
    assert!(super::authenticate(b"test", &STANDARD.encode(lines.join("\n")), &key).is_err());
    let forged = STANDARD.encode(SIG.replace("file:test", "file:test\tversion:0.3.0"));
    assert!(super::authenticate(b"test", &forged, &key).is_err());
    let mut packet = STANDARD.decode(KEY.lines().nth(1).unwrap()).unwrap();
    packet[10] ^= 1;
    let wrong = STANDARD.encode(format!(
        "untrusted comment: wrong key\n{}\n",
        STANDARD.encode(packet)
    ));
    assert!(super::authenticate(b"test", &sig, &wrong).is_err());
    assert!(super::verify(b"test", &sig, &key, "0.3.0").is_err());
}
#[test]
fn exact_single_version_token_required() {
    assert!(super::check_version("timestamp:1\tversion:0.3.0", "0.3.0").is_ok());
    for comment in [
        "version:0.3.0-evil",
        "version:0.2.0",
        "file:version:0.3.0",
        "version:0.3.0 version:0.2.0",
        "timestamp:1",
    ] {
        assert!(super::check_version(comment, "0.3.0").is_err());
    }
}
