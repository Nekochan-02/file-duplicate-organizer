mod commands;
mod scanner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::scan_folder,
            commands::get_file_preview,
            commands::delete_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
