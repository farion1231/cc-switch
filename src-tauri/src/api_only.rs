//! API-only mode: runs the embedded web server without any Tauri/GTK initialization.
//! Activated at build time via `--features api-only` (used in Docker).
//! No display server (Xvfb) required.

use crate::headless::run_headless;

pub fn run() -> ! {
    // Initialize logging from RUST_LOG env var
    env_logger::init();

    run_headless();
}
