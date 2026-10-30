// No console window next to the launcher on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    recomplauncher_lib::run()
}
