// Hides the extra console window on Windows release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nunc_pro_tune_lib::run();
}
