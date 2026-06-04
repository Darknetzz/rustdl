//! Injects `RUSTDL_BUILD_DATE` at compile time for the About dialog.
//! On Windows, embeds `assets/rustdl.ico` into the `.exe` (Explorer, shortcuts).

fn main() {
    let date = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    println!("cargo:rustc-env=RUSTDL_BUILD_DATE={date}");
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
