//! 授权防护命令：license_status / license_activate。
//!
//! 与 license-guard 的守卫语义一致：开发构建（debug）直接放行，
//! 仅发布构建（release，等价 Python 版 sys.frozen）启用校验。

use crate::license_guard as lg;
use tauri::State;

use crate::state::AppState;

#[derive(serde::Serialize)]
pub struct LicenseStatus {
    pub status: String,
    pub message: String,
}

fn message_for(status: &str) -> String {
    match status {
        lg::STATUS_OK => "已授权".to_string(),
        lg::STATUS_MISSING => "未检测到授权，请输入激活口令".to_string(),
        lg::STATUS_CORRUPTED => "授权文件损坏，请重新激活".to_string(),
        lg::STATUS_SIGNATURE_INVALID => "凭证无效（校验失败），请重新激活".to_string(),
        lg::STATUS_MACHINE_MISMATCH => "授权与本机不匹配，请重新激活".to_string(),
        lg::STATUS_EXPIRED => "授权已过期，请重新获取口令激活".to_string(),
        lg::STATUS_CLOCK_ROLLBACK => "检测到系统时间异常，请校正系统时间后重新激活".to_string(),
        lg::STATUS_FINGERPRINT_ERROR => "无法采集机器指纹，请联系支持".to_string(),
        _ => status.to_string(),
    }
}

fn status_result(status: &str) -> LicenseStatus {
    LicenseStatus {
        status: status.to_string(),
        message: message_for(status),
    }
}

/// 查询本地授权状态。debug 构建直接放行（开发不拦）。
#[tauri::command]
pub fn license_status(_state: State<AppState>) -> LicenseStatus {
    if cfg!(debug_assertions) {
        return status_result(lg::STATUS_OK);
    }
    match lg::check_license() {
        Ok(s) => status_result(s),
        Err(()) => status_result(lg::STATUS_FINGERPRINT_ERROR),
    }
}

/// 提交激活口令：调用验证服务器换取凭证并落盘。
/// 网络请求最长 10 秒，放到阻塞线程池避免卡住 UI。
#[tauri::command]
pub async fn license_activate(code: String) -> LicenseStatus {
    if cfg!(debug_assertions) {
        return status_result(lg::STATUS_OK);
    }
    let code = code.trim().to_string();
    if code.is_empty() {
        return LicenseStatus {
            status: "invalid_input".to_string(),
            message: "请输入激活口令".to_string(),
        };
    }
    let res = tauri::async_runtime::spawn_blocking(move || -> LicenseStatus {
        match lg::activate(&code) {
            Ok((payload_b64, signature_b64)) => match lg::save_license(&payload_b64, &signature_b64) {
                Ok(()) => LicenseStatus {
                    status: "ok".to_string(),
                    message: "激活成功".to_string(),
                },
                Err(e) => LicenseStatus {
                    status: "save_failed".to_string(),
                    message: format!("保存授权凭证失败: {e}"),
                },
            },
            Err(msg) => LicenseStatus {
                status: "activate_failed".to_string(),
                message: msg,
            },
        }
    })
    .await;
    res.unwrap_or_else(|_| LicenseStatus {
        status: "activate_failed".to_string(),
        message: "激活任务异常终止".to_string(),
    })
}
