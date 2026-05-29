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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
