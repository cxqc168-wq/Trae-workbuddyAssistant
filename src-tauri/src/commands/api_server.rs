use std::sync::{Arc, Mutex};

use tauri::State;

use crate::fs_utils;
use crate::models::{
    AccountCooldownsFile, AccountsFile, ApiPoolFile, ApiServiceStatus, DeviceMap, PoolStatus,
    RemainingCreditsFile,
};
use crate::state::AppState;

use crate::api_server::pool::ApiPool;
use crate::api_server::server::{start_api_server, ApiServerHandle};
use crate::api_server::{ApiLogger, ApiSharedState};

/// 运行时状态：服务器句柄 + 共享状态
pub struct ApiServerRuntime {
    pub handle: ApiServerHandle,
    pub shared: Arc<ApiSharedState>,
    pub started_at: u64,
}

/// 安全获取 Mutex 锁：若锁被毒化（panic 导致），仍恢复内部数据继续运行
fn safe_lock<'a, T>(m: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ==================== 启停命令 ====================

#[tauri::command]
pub async fn api_server_start(
    state: State<'_, AppState>,
    runtime: State<'_, Mutex<Option<ApiServerRuntime>>>,
) -> Result<ApiServiceStatus, String> {
    // 检查是否已运行
    {
        let guard = safe_lock(&runtime);
        if guard.is_some() {
            return Err("API 服务已在运行".into());
        }
    }

    let settings = state.settings();
    let port = settings.api_port;
    let api_key = settings.api_key.clone();

    // 关键：设置 NO_PROXY 环境变量，防止 ureq 走系统代理（127.0.0.1:8899）
    // 当代理开启时，系统代理会将 API 服务的上游请求也拦截，形成循环导致超时
    // ureq 2.x 在 AgentBuilder::build() 时读取 HTTP_PROXY/HTTPS_PROXY/NO_PROXY
    std::env::set_var("NO_PROXY", "*");
    std::env::set_var("no_proxy", "*");
    let default_model = {
        let m = settings.api_default_model.trim();
        if m.is_empty() {
            crate::api_server::DEFAULT_MODEL.to_string()
        } else {
            m.to_string()
        }
    };

    // 读取账号数据、冷却状态、剩余积分
    let accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let pool_file: ApiPoolFile = fs_utils::read_json(&state.path("api_pool.json"));
    let cooldowns_file: AccountCooldownsFile =
        fs_utils::read_json(&state.path("account_cooldowns.json"));
    let credits_file: RemainingCreditsFile =
        fs_utils::read_json(&state.path("remaining_credits.json"));
    let device_map: DeviceMap = fs_utils::read_json(&state.path("device_map.json"));

    // ===== 启动诊断日志：详细记录账号池资源情况 =====
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "API服务启动-账号池诊断: total_accounts={} enabled_in_pool={} cooldowns={} credits_entries={}",
            accounts.accounts.len(),
            pool_file.enabled_uids.len(),
            cooldowns_file.cooldowns.len(),
            credits_file.credits.len(),
        ),
    );

    // 逐账号诊断：哪些会被加入池，哪些会被跳过及原因
    let enabled_set: std::collections::HashSet<&str> =
        pool_file.enabled_uids.iter().map(|s| s.as_str()).collect();
    for a in &accounts.accounts {
        let uid = a.user_id.as_deref().unwrap_or("(none)");
        let name = &a.name;
        if !enabled_set.contains(uid) {
            fs_utils::app_log(
                &state.data_dir,
                &format!("  账号池跳过: name={} uid={} reason=not_in_enabled_list", name, uid),
            );
        } else {
            // 检查冷却和积分状态
            let cd = cooldowns_file.cooldowns.get(uid);
            let credits = credits_file.credits.get(uid).copied();
            let expire = credits_file.expire_times.get(uid).copied();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let status = if cd.map_or(false, |c| c.error_type == "SessionDead") {
                "SessionDead(disabled)".to_string()
            } else if cd.map_or(false, |c| c.until > 0 && now < c.until) {
                format!("cooldown(remaining={}s)", cd.unwrap().until - now)
            } else if let Some(exp) = expire {
                if exp > 0 && exp < now {
                    "credits_expired".to_string()
                } else if let Some(c) = credits {
                    if c <= 0.0 {
                        "zero_credits".to_string()
                    } else {
                        "healthy".to_string()
                    }
                } else {
                    "healthy(no_credits_info)".to_string()
                }
            } else {
                "healthy(no_expiry)".to_string()
            };
            fs_utils::app_log(
                &state.data_dir,
                &format!(
                    "  账号池纳入: name={} uid={} credits={} expire={} status={}",
                    name, uid,
                    credits.map(|c| format!("{:.0}", c)).unwrap_or_else(|| "None".into()),
                    expire.map(|e| e.to_string()).unwrap_or_else(|| "None".into()),
                    status,
                ),
            );
        }
    }

    // 创建池并同步
    let pool = ApiPool::new();
    pool.sync_from_accounts(
        &accounts.accounts,
        &pool_file.enabled_uids,
        &cooldowns_file.cooldowns,
        &credits_file.credits,
        &credits_file.expire_times,
        &device_map,
    );

    let pool_count = pool.count();
    let healthy_count = pool.diagnose().iter().filter(|d| d.reason.starts_with("healthy")).count();
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "API服务启动-池状态: pool_size={} healthy={} port={}",
            pool_count, healthy_count, port,
        ),
    );

    // 池为空时给出明确警告
    if pool_count == 0 {
        fs_utils::app_log(
            &state.data_dir,
            "警告: 账号池为空！请在API服务页面勾选账号并保存后再启动。当前 api_pool.json 中 enabled_uids 为空。",
        );
    } else if healthy_count == 0 {
        fs_utils::app_log(
            &state.data_dir,
            "警告: 池中无健康账号！所有账号可能处于冷却/积分过期/SessionDead 状态。请检查账号状态或清除冷却。",
        );
    }

    let shared = Arc::new(ApiSharedState {
        pool,
        api_key,
        default_model,
        total_requests: std::sync::atomic::AtomicU64::new(0),
        active_uid: Mutex::new(None),
        last_error: Mutex::new(None),
        logger: ApiLogger::new(state.logs_dir()),
        debug_enabled: std::sync::atomic::AtomicBool::new(false),
    });

    let handle = start_api_server(port, shared.clone()).await?;

    fs_utils::app_log(
        &state.data_dir,
        &format!("API 服务已启动: port={} pool_accounts={}", port, pool_count),
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let status = ApiServiceStatus {
        running: true,
        port,
        total_requests: 0,
        active_uid: None,
        last_error: None,
        started_at: Some(now),
    };

    *safe_lock(&runtime) = Some(ApiServerRuntime {
        handle,
        shared: shared.clone(),
        started_at: now,
    });

    Ok(status)
}

#[tauri::command]
pub async fn api_server_stop(
    state: State<'_, AppState>,
    runtime: State<'_, Mutex<Option<ApiServerRuntime>>>,
) -> Result<(), String> {
    let mut guard = safe_lock(&runtime);
    if let Some(mut rt) = guard.take() {
        rt.handle.stop();
        fs_utils::app_log(&state.data_dir, "API 服务已停止");
    }
    Ok(())
}

#[tauri::command]
pub fn api_server_status(
    state: State<'_, AppState>,
    runtime: State<'_, Mutex<Option<ApiServerRuntime>>>,
) -> ApiServiceStatus {
    let guard = safe_lock(&runtime);
    match guard.as_ref() {
        Some(rt) => {
            let total = rt
                .shared
                .total_requests
                .load(std::sync::atomic::Ordering::Relaxed);
            let active = safe_lock(&rt.shared.active_uid).clone();
            let last_err = safe_lock(&rt.shared.last_error).clone();
            ApiServiceStatus {
                running: true,
                port: state.settings().api_port,
                total_requests: total,
                active_uid: active,
                last_error: last_err,
                started_at: Some(rt.started_at),
            }
        }
        None => {
            let settings = state.settings();
            ApiServiceStatus {
                running: false,
                port: settings.api_port,
                total_requests: 0,
                active_uid: None,
                last_error: None,
                started_at: None,
            }
        }
    }
}

// ==================== 池管理命令 ====================

#[tauri::command]
pub fn pool_list(state: State<'_, AppState>) -> ApiPoolFile {
    fs_utils::read_json(&state.path("api_pool.json"))
}

/// 批量设置池中的账号 UID 列表
#[tauri::command]
pub fn pool_set(state: State<'_, AppState>, uids: Vec<String>) -> Result<(), String> {
    let pool_file = ApiPoolFile {
        enabled_uids: uids,
    };
    fs_utils::write_json(&state.path("api_pool.json"), &pool_file)
}

/// 返回运行中池的实时状态（冷却/积分等）；服务未运行时返回空数组
#[tauri::command]
pub fn pool_status(runtime: State<'_, Mutex<Option<ApiServerRuntime>>>) -> Vec<PoolStatus> {
    let guard = safe_lock(&runtime);
    match guard.as_ref() {
        Some(rt) => rt.shared.pool.status_list(),
        None => vec![],
    }
}

/// 列出 API 日志可用日期列表
#[tauri::command]
pub fn api_logs_list(state: State<'_, AppState>) -> Vec<String> {
    let logger = ApiLogger::new(state.logs_dir());
    logger.list_dates(30)
}

/// 读取指定日期的 API 日志内容
#[tauri::command]
pub fn api_logs_detail(state: State<AppState>, date: String) -> Option<String> {
    let logger = ApiLogger::new(state.logs_dir());
    logger.read_log(&date)
}

/// 按时间段和关键字搜索 API 日志
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiLogSearchOpts {
    pub date: String,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub keyword: Option<String>,
}

#[tauri::command]
pub fn api_logs_search(state: State<AppState>, opts: ApiLogSearchOpts) -> Option<String> {
    let logger = ApiLogger::new(state.logs_dir());
    logger.search_log(
        &opts.date,
        opts.start_time.as_deref().unwrap_or(""),
        opts.end_time.as_deref().unwrap_or(""),
        opts.keyword.as_deref().unwrap_or(""),
    )
}

/// 切换 API Debug 模式（开启后记录完整请求/响应）
#[tauri::command]
pub fn api_debug_toggle(
    runtime: State<'_, Mutex<Option<ApiServerRuntime>>>,
) -> Result<bool, String> {
    let guard = safe_lock(&runtime);
    match guard.as_ref() {
        Some(rt) => {
            let current = rt
                .shared
                .debug_enabled
                .load(std::sync::atomic::Ordering::Relaxed);
            let new_val = !current;
            rt.shared
                .debug_enabled
                .store(new_val, std::sync::atomic::Ordering::Relaxed);
            Ok(new_val)
        }
        None => Err("API 服务未运行".into()),
    }
}

/// 查询 API Debug 模式状态
#[tauri::command]
pub fn api_debug_status(
    runtime: State<'_, Mutex<Option<ApiServerRuntime>>>,
) -> bool {
    let guard = safe_lock(&runtime);
    match guard.as_ref() {
        Some(rt) => rt
            .shared
            .debug_enabled
            .load(std::sync::atomic::Ordering::Relaxed),
        None => false,
    }
}
