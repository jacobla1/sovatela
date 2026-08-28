// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before anything else. Started with the helper flag, this process exists
    // only to parse one PDF under a memory cap and hand back the text — it
    // must not open a window, touch the keychain, or build an HTTP client.
    if scale_lib::pdf_sandbox::run_helper_if_requested() {
        return;
    }
    scale_lib::run()
}
