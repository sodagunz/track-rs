use std::process::Command;

#[test]
fn invalid_theme_exits_before_audio_or_terminal_setup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("主题.json");
    std::fs::write(&path, r#"{"accent":"not-a-color"}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_track-rs"))
        .arg("--theme")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("parse theme"), "{stderr}");
    assert!(stderr.contains("invalid color"), "{stderr}");
    assert!(stderr.contains("主题.json"), "{stderr}");
    assert!(!stderr.contains("ALSA"), "{stderr}");
    assert!(!output.stdout.contains(&0x1b));
}

#[test]
fn explicit_missing_theme_and_argument_errors_precede_startup() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    let output = Command::new(env!("CARGO_BIN_EXE_track-rs"))
        .arg("--theme")
        .arg(missing)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("read theme"));
    let output = Command::new(env!("CARGO_BIN_EXE_track-rs"))
        .arg("--theme")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--theme requires a path"));
}
