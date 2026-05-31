mod commands;

use commands::manifest::*;
use commands::platform::*;
use commands::software::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            detect_platform,
            refresh_manifest,
            refresh_scripts,
            get_manifest_source,
            get_platform_catalog,
            detect_installed,
            check_latest,
            install_software,
            upgrade_software,
        ])
        .setup(|app| {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    // Optimized wheel event handler for WebKitGTK
                    window.eval("
                        (function() {
                            let rafId = null;
                            let scrollY = 0;

                            window.addEventListener('wheel', function(e) {
                                scrollY += e.deltaY;

                                if (!rafId) {
                                    rafId = requestAnimationFrame(function() {
                                        // Use window.scrollBy for smoother scrolling
                                        window.scrollBy(0, scrollY);
                                        scrollY = 0;
                                        rafId = null;
                                    });
                                }

                                e.preventDefault();
                            }, { passive: false });
                        })();
                    ").ok();
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
