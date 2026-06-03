//! Windows uses the default console subsystem so CLI modes work in terminals.
//! The GUI path calls [`rustdl::cli::detach_console_for_gui`] to avoid a stray console window.

fn main() {
    rustdl::main_entry();
}
