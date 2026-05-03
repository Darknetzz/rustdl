//! Injects `RUSTDL_BUILD_DATE` at compile time for the About dialog.
fn main() {
    let date = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    println!("cargo:rustc-env=RUSTDL_BUILD_DATE={date}");
    println!("cargo:rerun-if-changed=build.rs");
}
