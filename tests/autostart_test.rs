use byte::autostart;

#[test]
fn the_location_is_described_for_this_platform() {
    let described = autostart::describe_location();
    assert!(
        !described.trim().is_empty(),
        "users need to know where byte would install itself"
    );
    #[cfg(windows)]
    assert!(
        described.contains("Run") || described.to_lowercase().contains("registry"),
        "should name the registry Run key: {described}"
    );
    #[cfg(target_os = "macos")]
    assert!(
        described.to_lowercase().contains("launchagent"),
        "should name the LaunchAgent: {described}"
    );
}

#[test]
fn status_is_readable_without_changing_anything() {
    // Must not panic and must not enable anything as a side effect.
    let before = autostart::status();
    let after = autostart::status();
    assert_eq!(before.is_ok(), after.is_ok(), "status must be a pure read");
    if let (Ok(a), Ok(b)) = (before, after) {
        assert_eq!(a, b, "status must not change between consecutive reads");
    }
}
