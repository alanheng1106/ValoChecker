pub mod api;
pub mod auth;
pub mod store;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_local_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let store = store::StoreManager::new(app_data_dir);
            let app_state = auth::AppState::new(store);
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::check_login,
            auth::start_webview_login,
            auth::logout,
            api::get_storefront,
            api::get_match_history,
            api::get_match_details,
            api::get_skin_details
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
