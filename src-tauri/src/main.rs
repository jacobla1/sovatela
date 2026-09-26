// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before anything else. Started with the helper flag, this process exists
    // only to parse one document under a memory cap and hand back the text — it
    // must not open a window, touch the keychain, or build an HTTP client.
    if scale_lib::doc_sandbox::run_helper_if_requested() {
        return;
    }
    // Validation only, and only in a testhooks build: the self-test and probes
    // the Windows gate measures confinement with. A build that could ship has
    // none of them, and their flags fall through to the refusal below.
    #[cfg(all(windows, feature = "windows-confinement-testhooks"))]
    {
        use scale_lib::doc_confinement_windows::validation as v;
        if v::run_selftest_if_requested()
            || v::run_probe_if_requested()
            || v::run_confined_probe_if_requested()
            || v::run_handle_probe_if_requested()
            || v::run_confined_handle_probe_if_requested()
            || v::run_descendant_child_if_requested()
            || v::run_confined_descendant_probe_if_requested()
            || v::run_acl_probe_if_requested()
            || v::run_exec_child_if_requested()
            || v::run_confined_exec_probe_if_requested()
        {
            return;
        }
    }

    // An internal flag that nothing claimed must not start the application.
    //
    // It did. A self-test invocation whose flag went unrecognised fell through
    // to here, Tauri tried to open a window on a headless CI runner, and the
    // job sat for three minutes and printed nothing — a hang that looked like
    // the sandbox failing and was the argument parser missing.
    if let Some(flag) = std::env::args().nth(1) {
        if flag.starts_with("--sovatela-") {
            eprintln!("unrecognised internal flag: {flag}");
            eprintln!("arguments: {:?}", std::env::args().collect::<Vec<_>>());
            std::process::exit(64);
        }
    }

    scale_lib::run()
}
