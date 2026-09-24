mod commands;

use commands::auth::*;
use commands::entitlements::*;
use commands::manifest::*;
use commands::platform::*;
use commands::software::*;
use commands::supervisor::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            detect_platform,
            open_external_url,
            refresh_manifest,
            refresh_scripts,
            get_manifest_source,
            get_platform_catalog,
            detect_installed,
            check_latest,
            check_software,
            install_software,
            upgrade_software,
            supervisor_status,
            supervisor_restart,
            supervisor_tail_log,
            auth_status,
            auth_login,
            auth_logout,
            refresh_entitlements,
            get_cached_entitlements,
        ])
        .setup(|_app| {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                let app = _app;
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
