//! Desktop entry point.

// Hides the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started by `ssh` as `SSH_ASKPASS`: answer the prompt and exit before any UI exists.
    if rd_ssh::askpass::client::helper_requested() {
        std::process::exit(rd_ssh::askpass::client::run_helper());
    }
    rd_app_lib::run();
}
