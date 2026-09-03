// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod fs_utils;
mod jwt;
mod license_guard;
mod models;
mod python;
mod state;
mod workbuddy;
mod api_server;

use state::AppState;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

fn main() {
    let state = match AppState::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("初始化失败: {e}");
            std::process::exit(1);
        }
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(state)
        .manage(Mutex::new(Option::<commands::proxy::ProxyHandle>::None))
        .manage(Mutex::new(Option::<commands::api_server::ApiServerRuntime>::None))
        .manage(Mutex::new(Option::<commands::oauth::OAuthCallbackHandle>::None))
        .invoke_handler(tauri::generate_handler![
            commands::license::license_status,
            commands::license::license_activate,
            commands::env::env_check,
            commands::env::open_trae_website,
            commands::env::open_trae_app,
            commands::cert::cert_status,
            commands::cert::cert_install,
            commands::proxy::proxy_start,
            commands::proxy::proxy_stop,
            commands::proxy::proxy_status,
            commands::accounts::accounts_list,
            commands::accounts::accounts_export_raw,
            commands::accounts::account_add_manual,
            commands::accounts::account_delete,
            commands::accounts::account_update,
            commands::accounts::groups_list,
            commands::accounts::group_create,
            commands::accounts::group_update,
            commands::accounts::group_delete,
            commands::accounts::group_move,
            commands::accounts::fetch_remaining_credits,
            commands::accounts::refresh_remaining_credits,
            commands::accounts::credits_daily_list,
            commands::accounts::cooldown_clear,
            commands::accounts::cooldown_clear_all,
            commands::accounts::refresh_jwt,
            commands::checkin::checkin_start,
            commands::switch::switch_account,
            commands::switch::save_current_login,
            commands::switch::reset_device_ids,
            commands::misc::device_reset,
            commands::misc::jwt_parse,
            commands::misc::logs_query,
            commands::misc::settings_get,
            commands::misc::settings_set,
            commands::misc::invite_link,
            commands::misc::credits_history,
            commands::misc::task_register,
            commands::misc::task_status,
            commands::misc::task_unregister,
            commands::misc::proxy_logs_list,
            commands::misc::proxy_log_detail,
            commands::misc::write_text_file,
            commands::api_server::api_server_start,
            commands::api_server::api_server_stop,
            commands::api_server::api_server_status,
            commands::api_server::pool_list,
            commands::api_server::pool_set,
            commands::api_server::pool_status,
            commands::api_server::api_logs_list,
            commands::api_server::api_logs_detail,
            commands::api_server::api_logs_search,
            commands::api_server::api_debug_toggle,
            commands::api_server::api_debug_status,
            commands::profile::profile_list,
            commands::profile::profile_backup,
            commands::profile::profile_restore,
            commands::profile::profile_delete,
            commands::profile::profile_format_size,
            commands::oauth::oauth_get_login_url,
            commands::oauth::oauth_parse_callback,
            commands::oauth::oauth_login,
            commands::oauth::oauth_callback_start,
            commands::oauth::oauth_callback_stop,
            commands::workbuddy::workbuddy_list_accounts,
            commands::workbuddy::workbuddy_client_status,
            commands::workbuddy::workbuddy_import_local,
            commands::workbuddy::workbuddy_add_manual,
            commands::workbuddy::workbuddy_delete_account,
            commands::workbuddy::workbuddy_checkin_status,
            commands::workbuddy::workbuddy_checkin_all,
            commands::workbuddy::workbuddy_credits,
            commands::workbuddy::workbuddy_refresh_token,
            commands::workbuddy::workbuddy_oauth_start,
            commands::workbuddy::workbuddy_oauth_poll,
        ])
        .setup(|app| {
            let state = app.state::<AppState>();
            let settings = state.settings();

            // 启动期日志清理：按 log_retention_days 丢弃过期日志行（消费设置项，避免无限增长）
            let retention = settings.log_retention_days.max(0) as u64;
            fs_utils::trim_logs(&state.data_dir, retention);

            fs_utils::app_log(
                &state.data_dir,
                &format!(
                    "应用启动: tray=enabled, launch_minimized={}, auto_start_proxy={}",
                    settings.launch_minimized, settings.auto_start_proxy
                ),
            );

            // 创建系统托盘（始终启用，支持最小化到托盘；失败不阻断启动）
            {
                let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                    let toggle_item =
                        MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
                    let sep = PredefinedMenuItem::separator(app)?;
                    let quit_item =
                        MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
                    let menu = Menu::with_items(app, &[&toggle_item, &sep, &quit_item])?;

                    let icon = app
                        .default_window_icon()
                        .cloned()
                        .ok_or("找不到默认窗口图标")?;

                    TrayIconBuilder::new()
                        .icon(icon)
                        .tooltip("Trae Work 助手")
                        .menu(&menu)
                        .on_tray_icon_event(|tray, event| {
                            if let TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            } = event
                            {
                                let app = tray.app_handle();
                                let st = app.state::<AppState>();
                                if let Some(window) = app.get_webview_window("main") {
                                    if window.is_visible().unwrap_or(false) {
                                        let _ = window.hide();
                                        fs_utils::app_log(&st.data_dir, "托盘左键点击：隐藏窗口");
                                    } else {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                        fs_utils::app_log(&st.data_dir, "托盘左键点击：显示窗口");
                                    }
                                } else {
                                    fs_utils::app_log(&st.data_dir, "托盘左键点击：找不到主窗口");
                                }
                            }
                        })
                        .on_menu_event(|app, event| match event.id.as_ref() {
                            "toggle" => {
                                let st = app.state::<AppState>();
                                if let Some(window) = app.get_webview_window("main") {
                                    if window.is_visible().unwrap_or(false) {
                                        let _ = window.hide();
                                        fs_utils::app_log(&st.data_dir, "菜单：隐藏窗口");
                                    } else {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                        fs_utils::app_log(&st.data_dir, "菜单：显示窗口");
                                    }
                                }
                            }
                            "quit" => {
                                let st = app.state::<AppState>();
                                fs_utils::app_log(&st.data_dir, "菜单：用户请求退出应用");
                                app.exit(0);
                            }
                            _ => {}
                        })
                        .build(app)?;
                    Ok(())
                })();

                match &result {
                    Ok(()) => fs_utils::app_log(&state.data_dir, "托盘图标创建成功"),
                    Err(e) => {
                        let msg = format!("托盘创建失败（应用将继续运行）: {e}");
                        fs_utils::app_log(&state.data_dir, &msg);
                        eprintln!("{msg}");
                    }
                }
            }

            // 启动时最小化到托盘
            if settings.launch_minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                    fs_utils::app_log(&state.data_dir, "启动最小化：窗口已隐藏到托盘");
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭即退出应用（前端已弹确认框；退出时 RunEvent::Exit 自动清理代理与 API 服务）
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let st = window.app_handle().state::<AppState>();
                fs_utils::app_log(&st.data_dir, "用户确认退出应用");
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            // 应用退出时清理代理子进程，防止端口占用
            let proxy_state = app_handle.state::<Mutex<Option<commands::proxy::ProxyHandle>>>();
            let mut g = proxy_state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(h) = g.take() {
                let state = app_handle.state::<AppState>();
                fs_utils::app_log(&state.data_dir, "应用退出：正在清理代理子进程");
                drop(h); // Drop trait 会 kill + wait 子进程
            }
            // 应用退出时停止 API 服务
            let api_state = app_handle
                .state::<Mutex<Option<commands::api_server::ApiServerRuntime>>>();
            let mut ag = api_state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(mut rt) = ag.take() {
                let state = app_handle.state::<AppState>();
                fs_utils::app_log(&state.data_dir, "应用退出：正在停止 API 服务");
                rt.handle.stop();
            }
            // 还原系统代理，避免退出后本机全局断网
            if let Err(e) = commands::proxy::clear_win_proxy() {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    fs_utils::app_log(&state.data_dir, &format!("应用退出：还原系统代理失败(可手动关闭): {e}"));
                }
            }
        }
    });
}
