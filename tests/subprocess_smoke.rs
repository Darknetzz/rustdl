//! Optional subprocess smoke test. Run with `RUSTDL_IT=1 cargo test --test subprocess_smoke -- --ignored`.

#[cfg(windows)]
#[test]
#[ignore = "requires yt-dlp on PATH; set RUSTDL_IT=1 to run"]
fn yt_dlp_spawn_after_detach_console() {
    if std::env::var("RUSTDL_IT").ok().as_deref() != Some("1") {
        eprintln!("skip: set RUSTDL_IT=1 to run subprocess integration tests");
        return;
    }
    rustdl::cli::detach_console_for_gui();
    let status = std::process::Command::new("yt-dlp")
        .arg("--version")
        .status()
        .expect("spawn yt-dlp after detach_console_for_gui");
    assert!(status.success(), "yt-dlp --version failed");
}

#[test]
#[ignore = "requires yt-dlp on PATH; set RUSTDL_IT=1 to run"]
fn yt_dlp_version_on_path() {
    if std::env::var("RUSTDL_IT").ok().as_deref() != Some("1") {
        eprintln!("skip: set RUSTDL_IT=1 to run subprocess integration tests");
        return;
    }
    let status = std::process::Command::new("yt-dlp")
        .arg("--version")
        .status()
        .expect("spawn yt-dlp");
    assert!(status.success(), "yt-dlp --version failed");
}
