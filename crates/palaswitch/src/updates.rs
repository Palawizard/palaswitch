//! Installation and updates, through the Velopack installer.

use velopack::VelopackApp;

use crate::autostart::RUN_KEY;

/// Answers the installer when it starts the app for one of its hooks, and
/// exits then; returns at once on a normal start, or when the app runs
/// outside an installation.
///
/// Must run before the single-instance lock: the installer starts the app
/// while another copy may be running.
pub fn run_installer_hooks() {
    VelopackApp::build()
        // A downloaded update is applied when the user asks for it, never
        // by a second launch that only meant to open the settings.
        .set_auto_apply_on_startup(false)
        .on_before_uninstall_fast_callback(|_| {
            let _ = RUN_KEY.forget();
        })
        .run();
}
