//! Trae 登录态切换命令（JWT 注入方式）

use tauri::{AppHandle, Emitter, State};

use crate::fs_utils;
use crate::models::RawAccount;
use crate::state::AppState;
use crate::trae_auth::{read_current_login, switch_account, TraeLoginInfo};

/// 切换到指定账号（JWT 注入方式，替换旧的快照方式）
///
/// 从账号存储中读取目标账号的 JWT 和 refresh_token，直接注入到 Trae 的 storage.json 中
#[tauri::command(async)]
pub fn trae_switch_account(
    app: AppHandle,
    state: State<AppState>,
    user_id: String,
) -> Result<(), String> {
    fs_utils::app_log(
        &state.data_dir,
        &format!("trae_switch_account: user_id={user_id}"),
    );

    // 发送开始事件
    let _ = app.emit(
        "switch-progress",
        serde_json::to_string(&serde_json::json!({ "stage": "start", "message": format!("准备切换到账号 {user_id}") })).unwrap_or_default(),
    );

    // 将预检查也纳入完成事件，保证每条失败路径都结束前端的切换状态。
    let result = (|| {
        // 从账号存储中查找目标账号
        let accounts: crate::models::AccountsFile =
            fs_utils::read_json_strict(&state.path("checkin_accounts.json"))?;
        let account = accounts
            .accounts
            .iter()
            .find(|a| {
                a.user_id.as_deref() == Some(&user_id)
                    || crate::jwt::parse(&a.jwt).user_id.as_deref() == Some(&user_id)
            })
            .ok_or_else(|| format!("账号 {user_id} 不存在"))?;

        if account.jwt.is_empty() {
            return Err(format!("账号 {user_id} 没有有效的 JWT"));
        }

        let _ = app.emit(
            "switch-progress",
            serde_json::to_string(
                &serde_json::json!({ "stage": "kill", "message": "正在关闭 Trae..." }),
            )
            .unwrap_or_default(),
        );

        // 构建登录信息
        let login_info = TraeLoginInfo {
            token: account.jwt.clone(),
            refresh_token: account.refresh_token.clone(),
            user_id: user_id.clone(),
            email: String::new(),
            username: account.name.clone(),
            avatar_url: String::new(),
        };

        let settings = state.settings();
        let custom_exe = settings.trae_path.as_deref();

        // 执行切换
        switch_account(&login_info, &state.data_dir, custom_exe)
    })();
    match result {
        Ok(_) => {
            let _ = app.emit(
                "switch-progress",
                serde_json::to_string(&serde_json::json!({ "stage": "done", "message": "目标账号登录态已写入，Trae 已启动" })).unwrap_or_default(),
            );
            let _ = app.emit(
                "switch-done",
                serde_json::json!({ "success": true, "raw": "done" }),
            );
            fs_utils::app_log(&state.data_dir, "trae_switch_account: 切换成功");
            Ok(())
        }
        Err(e) => {
            let _ = app.emit(
                "switch-progress",
                serde_json::to_string(
                    &serde_json::json!({ "stage": "fatal", "message": format!("切换失败: {e}") }),
                )
                .unwrap_or_default(),
            );
            let _ = app.emit(
                "switch-done",
                serde_json::json!({ "success": false, "raw": format!("error: {e}") }),
            );
            fs_utils::app_log(
                &state.data_dir,
                &format!("trae_switch_account: 切换失败: {e}"),
            );
            Err(e)
        }
    }
}

/// 保存当前 Trae 的登录态到账号存储
///
/// 从 Trae 的 storage.json 中读取当前登录信息，保存到指定账号
#[tauri::command]
pub fn trae_save_current_login(
    app: AppHandle,
    state: State<AppState>,
    user_id: String,
) -> Result<(), String> {
    fs_utils::app_log(
        &state.data_dir,
        &format!("trae_save_current_login: user_id={user_id}"),
    );

    let _ = app.emit(
        "save-login-progress",
        serde_json::to_string(
            &serde_json::json!({ "stage": "start", "message": "正在读取当前登录态..." }),
        )
        .unwrap_or_default(),
    );

    // 从 Trae 读取当前登录信息
    let login_info = read_current_login().map_err(|e| format!("读取当前登录态失败: {e}"))?;

    let _ = app.emit(
        "save-login-progress",
        serde_json::to_string(&serde_json::json!({ "stage": "read", "message": format!("已读取账号 {} 的登录态", login_info.user_id) })).unwrap_or_default(),
    );

    // 更新到账号存储
    let accounts_path = state.path("checkin_accounts.json");
    let mut accounts: crate::models::AccountsFile = fs_utils::read_json(&accounts_path);

    if let Some(account) = accounts
        .accounts
        .iter_mut()
        .find(|a| a.user_id.as_deref() == Some(&user_id))
    {
        account.jwt = login_info.token;
        account.refresh_token = login_info.refresh_token;
        account.user_id = Some(login_info.user_id.clone());
        account.name = if login_info.username.is_empty() {
            login_info.email.clone()
        } else {
            login_info.username.clone()
        };
        account.updated_at = Some(chrono::Local::now().to_rfc3339());
    } else {
        // 账号不存在，创建新账号
        let new_account = RawAccount {
            id: Some(format!("acc-{}", uuid::Uuid::new_v4().simple())),
            name: if login_info.username.is_empty() {
                login_info.email.clone()
            } else {
                login_info.username.clone()
            },
            user_id: Some(login_info.user_id.clone()),
            jwt: login_info.token,
            refresh_token: login_info.refresh_token,
            added_at: Some(chrono::Local::now().to_rfc3339()),
            updated_at: Some(chrono::Local::now().to_rfc3339()),
        };
        accounts.accounts.push(new_account);
    }

    fs_utils::write_json(&accounts_path, &accounts)
        .map_err(|e| format!("保存账号数据失败: {e}"))?;

    let _ = app.emit(
        "save-login-progress",
        serde_json::to_string(&serde_json::json!({ "stage": "done", "message": "登录态已保存" }))
            .unwrap_or_default(),
    );
    let _ = app.emit(
        "save-login-done",
        serde_json::json!({ "success": true, "raw": "done" }),
    );

    fs_utils::app_log(&state.data_dir, "trae_save_current_login: 保存成功");
    Ok(())
}
