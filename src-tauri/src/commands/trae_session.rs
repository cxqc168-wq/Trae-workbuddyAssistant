//! Trae 对话解密导出命令。

use crate::state::AppState;
use crate::trae_session;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct TraeSessionListResult {
    pub plain_db_path: String,
    pub sessions: Vec<trae_session::TraeChatSession>,
}

/// 扫描 Trae 进程内存获取密钥，解密数据库，返回会话列表。
#[tauri::command(async)]
pub async fn trae_session_list(state: State<'_, AppState>) -> Result<TraeSessionListResult, String> {
    let output_dir = state.data_dir.join("trae_export");
    let (plain_path, sessions) = trae_session::trae_session_list(&output_dir)?;
    crate::fs_utils::app_log(
        &state.data_dir,
        &format!("Trae 对话已解密，共 {} 个会话", sessions.len()),
    );
    Ok(TraeSessionListResult {
        plain_db_path: plain_path.to_string_lossy().to_string(),
        sessions,
    })
}

/// 导出指定会话的完整消息为 JSON 文件，返回文件路径。
#[tauri::command(async)]
pub async fn trae_session_export_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let output_dir = state.data_dir.join("trae_export");
    let plain_db = output_dir.join("trae_plain.db");
    if !plain_db.exists() {
        return Err("请先执行「扫描并解密」生成明文数据库".into());
    }
    let messages = trae_session::export_session_messages(&plain_db, &session_id)?;
    let json = serde_json::to_string_pretty(&messages)
        .map_err(|e| format!("序列化失败: {e}"))?;
    let out_path = output_dir.join(format!("trae_chat_{}.json", session_id));
    std::fs::write(&out_path, json).map_err(|e| format!("写入失败: {e}"))?;
    crate::fs_utils::app_log(
        &state.data_dir,
        &format!("Trae 会话消息已导出: {} ({} 条)", session_id, messages.len()),
    );
    Ok(out_path.to_string_lossy().to_string())
}
