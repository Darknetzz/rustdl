//! Injects `RUSTDL_BUILD_DATE` at compile time for the About dialog.
//! On Windows, embeds `assets/rustdl.ico` into the `.exe` (Explorer, shortcuts).

fn main() {
    let now = chrono::Utc::now();
    println!("cargo:rustc-env=RUSTDL_BUILD_UNIX={}", now.timestamp());
    println!(
        "cargo:rustc-env=RUSTDL_BUILD_DATE={}",
        now.format("%Y-%m-%d %H:%M UTC")
    );
    // No `rerun-if-changed=build.rs` — that pinned BUILD_DATE until build.rs itself changed.
    // Default: re-run this script whenever any package file changes.

    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/rustdl.ico");
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/rustdl.ico");
        res.compile()
            .expect("failed to embed Windows executable icon");
    }
}
