mod atomic_file;
mod commands;
mod converter;
mod diagnostics;
mod index_runtime;
mod knowledge_index;
mod logging;
mod mineru;
mod models;
mod runtime;
mod state;
mod storage;
mod tag_runtime;
mod tagging;

use commands::{
    apply_tagging_config, get_dashboard, get_diagnostic_report, get_tag_jobs,
    get_third_party_licenses, open_managed_path, open_mineru_token_page, preview_tagging_change,
    rescan_all_profiles, retry_failed_tasks, retry_tag_job, retry_tag_jobs, retry_task,
    run_health_check, save_agent_settings, save_settings, set_classification_paused,
    set_mineru_token, set_monitoring_paused, set_paused, test_agent_connection,
};
use state::AppState;
use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            state::migrate_legacy_data_dir(&data_dir)?;
            logging::initialize(&data_dir)?;
            let state = AppState::new(data_dir)?;
            runtime::start(state.clone())?;
            tag_runtime::start(state.clone())?;
            index_runtime::start(state.clone())?;
            app.manage(state);

            let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("CPAH Docs")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard,
            run_health_check,
            get_diagnostic_report,
            get_third_party_licenses,
            rescan_all_profiles,
            retry_failed_tasks,
            open_managed_path,
            open_mineru_token_page,
            save_settings,
            set_mineru_token,
            set_paused,
            set_monitoring_paused,
            set_classification_paused,
            retry_task,
            save_agent_settings,
            test_agent_connection,
            preview_tagging_change,
            apply_tagging_config,
            get_tag_jobs,
            retry_tag_job,
            retry_tag_jobs
        ])
        .run(tauri::generate_context!())
        .expect("error while running CPAH Docs");
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
