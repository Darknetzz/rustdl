//! Avoid a console window when launched from Explorer (Windows GUI subsystem).
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    rustdl::main_entry();
}
