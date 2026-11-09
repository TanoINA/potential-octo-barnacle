#[test]
fn shell_has_a_native_entrypoint_and_isolated_profile_startup() {
    let main = include_str!("../src/main.rs").to_string();
    assert!(
        main.contains("configure_storage_root"),
        "native shell must configure its isolated profile before constructing the controller"
    );
    assert!(
        main.contains("rune::"),
        "shell must launch Rune, not a web host"
    );
}
