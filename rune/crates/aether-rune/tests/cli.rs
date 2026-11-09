#[path = "../src/cli.rs"]
mod cli;
#[test]
fn cli_requires_explicit_import_and_never_consumes_a_missing_path() {
    let defaults = cli::Options::parse(&[]).unwrap();
    assert_eq!(defaults.profile, None);
    assert_eq!(defaults.import_profile, None);
    assert!(cli::Options::parse(&["--profile".into(), "--smoke".into()]).is_err());
    assert!(
        cli::Options::parse(&["--smoke".into(), "--profile".into(), "C:/unsafe".into()]).is_err()
    );
}
