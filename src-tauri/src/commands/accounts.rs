use serde::Serialize;
use tauri::State;

use crate::fs_utils;
use crate::jwt;
use crate::models::{
    AccountView, AccountsFile, DeviceMap, DeviceEntry, GroupsFile, Group, RawAccount,
    CreditsFile, CreditsDailyFile, CreditsDailySnapshot, CheckinSummary, RemainingCreditsFile, AccountCooldownsFile,
};

use crate::state::AppState;

// ---------------- 双 HTTP Client 设计 ----------------

/// 短请求 Agent：总超时 120s，用于签到/积分查询/Token 刷新等 JSON 请求
fn short_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .max_idle_connections(20)
        .max_idle_connections_per_host(20)
        .build()
}

/// 流式 Agent：无总超时，仅 response_header_timeout 120s，用于 SSE 流式对话
/// 预留给 Phase 3 OpenAI 兼容 API 使用
#[allow(dead_code)]
fn streaming_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        // 不设置 timeout_read（Duration::from_secs(0) 会触发 std 错误）
        .max_idle_connections(20)
        .max_idle_connections_per_host(20)
        .build()
}

#[tauri::command]
pub fn accounts_list(state: State<AppState>) -> Vec<AccountView> {
    build_account_views(&state)
}

/// 导出所有账号原始数据，字段名对齐参考 JSON（camelCase），供前端一键导出使用。
#[tauri::command]
pub fn accounts_export_raw(state: State<AppState>) -> Result<serde_json::Value, String> {
    let accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    let device_map: DeviceMap = fs_utils::read_json(&state.path("device_map.json"));
    let views = build_account_views(&state);

    let merged: Vec<serde_json::Value> = views
        .iter()
        .map(|v| {
            let raw = accounts
                .accounts
                .iter()
                .find(|a| a.user_id.as_deref() == Some(&v.user_id));
            let refresh_token = raw
                .and_then(|a| a.refresh_token.clone())
                .unwrap_or_default();
            let has_rt = !refresh_token.is_empty();

            let device_id = device_map
                .get(&v.user_id)
                .map(|d| d.device_id.clone())
                .unwrap_or_default();

            let jwt_source = if has_rt { "session" } else { "manual" };

            serde_json::json!({
                "name": v.name,
                "cloudIdeJwt": v.jwt,
                "deviceId": device_id,
                "jwtExp": v.jwt_exp_timestamp,
                "balance": v.credits,
                "refreshToken": refresh_token,
                "jwtSource": jwt_source,
                "userId": v.user_id,
                "groupId": v.group_id,
                "jwtExpHours": v.jwt_exp_hours,
                "checkedToday": v.checked_today,
                "remainingCredits": v.remaining_credits,
                "deviceIdMasked": v.device_id_masked,
                "cooldownType": v.cooldown_type,
                "cooldownUntil": v.cooldown_until,
                "cooldownReason": v.cooldown_reason,
                "hasRefreshToken": v.has_refresh_token,
                "jwtAutoRefresh": v.jwt_auto_refresh,
                "creditsExpireAt": v.credits_expire_at,
            })
        })
        .collect();

    let groups_arr: Vec<serde_json::Value> = groups
        .groups
        .iter()
        .map(|g| {
            serde_json::json!({
                "id": g.id,
                "name": g.name,
                "color": g.color,
                "order": g.order,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "exportedAt": fs_utils::now_iso(),
        "appVersion": "2.4.4",
        "accountCount": merged.len(),
        "accounts": merged,
        "groups": groups_arr,
    }))
}

#[tauri::command]
pub fn account_add_manual(
    state: State<AppState>,
    name: String,
    jwt: String,
    group_id: Option<String>,
) -> Result<(), String> {
    let info = jwt::parse(&jwt);
    let uid = info.user_id.ok_or("无法从 JWT 解析 UserID，请检查格式")?;
    let mut accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    if accounts
        .accounts
        .iter()
        .any(|a| a.user_id.as_deref() == Some(&uid))
    {
        return Err("该账号已存在".into());
    }
    accounts.accounts.push(RawAccount {
        name: name.clone(),
        user_id: Some(uid.clone()),
        jwt,
        refresh_token: None,
        added_at: Some(fs_utils::now_iso()),
        updated_at: Some(fs_utils::now_iso()),
    });
    fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;
    if let Some(g) = group_id {
        let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
        groups.membership.insert(uid, g);
        fs_utils::write_json(&state.path("groups.json"), &groups)?;
    }
    Ok(())
}

#[tauri::command]
pub fn account_delete(
    state: State<AppState>,
    user_id: String,
    delete_profile: bool,
) -> Result<(), String> {
    let mut accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    accounts
        .accounts
        .retain(|a| a.user_id.as_deref() != Some(user_id.as_str()));
    fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;

    let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    groups.membership.remove(&user_id);
    fs_utils::write_json(&state.path("groups.json"), &groups)?;

    if delete_profile {
        let p = state.path("profiles").join(&user_id);
        let _ = std::fs::remove_dir_all(p);
    }
    Ok(())
}

#[tauri::command]
pub fn account_update(
    state: State<AppState>,
    user_id: String,
    name: Option<String>,
    jwt: Option<String>,
) -> Result<(), String> {
    let mut accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let a = accounts
        .accounts
        .iter_mut()
        .find(|a| a.user_id.as_deref() == Some(user_id.as_str()))
        .ok_or("账号不存在")?;

    if let Some(n) = name {
        let n = n.trim().to_string();
        if !n.is_empty() {
            a.name = n;
        }
    }
    if let Some(j) = jwt {
        let j = j.trim().to_string();
        if !j.is_empty() {
            // 更新 JWT 后同步 user_id（JWT 可能换了账号）
            let info = crate::jwt::parse(&j);
            if let Some(uid) = info.user_id {
                a.user_id = Some(uid);
            }
            a.jwt = j;
        }
    }
    a.updated_at = Some(fs_utils::now_iso());
    fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;
    Ok(())
}

// ---------------- 分组 ----------------

#[derive(Serialize)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub color: String,
    pub order: i32,
    pub count: usize,
}

#[tauri::command]
pub fn groups_list(state: State<AppState>) -> Vec<GroupView> {
    let groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    groups
        .groups
        .iter()
        .map(|g| {
            let count = groups
                .membership
                .values()
                .filter(|v| *v == &g.id)
                .count();
            GroupView {
                id: g.id.clone(),
                name: g.name.clone(),
                color: g.color.clone(),
                order: g.order,
                count,
            }
        })
        .collect()
}

#[tauri::command]
pub fn group_create(state: State<AppState>, name: String, color: String) -> Result<String, String> {
    let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    let id = format!("g_{}", chrono::Local::now().timestamp_millis());
    let order = (groups.groups.len() as i32) + 1;
    groups.groups.push(Group {
        id: id.clone(),
        name,
        color,
        order,
    });
    fs_utils::write_json(&state.path("groups.json"), &groups)?;
    Ok(id)
}

#[tauri::command]
pub fn group_update(
    state: State<AppState>,
    id: String,
    name: Option<String>,
    color: Option<String>,
    order: Option<i32>,
) -> Result<(), String> {
    let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    let g = groups
        .groups
        .iter_mut()
        .find(|g| g.id == id)
        .ok_or("分组不存在")?;
    if let Some(n) = name {
        g.name = n;
    }
    if let Some(c) = color {
        g.color = c;
    }
    if let Some(o) = order {
        g.order = o;
    }
    fs_utils::write_json(&state.path("groups.json"), &groups)?;
    Ok(())
}

#[tauri::command]
pub fn group_delete(state: State<AppState>, id: String) -> Result<(), String> {
    let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    groups.groups.retain(|g| g.id != id);
    groups.membership.retain(|_, v| *v != id);
    fs_utils::write_json(&state.path("groups.json"), &groups)?;
    Ok(())
}

#[tauri::command]
pub fn group_move(
    state: State<AppState>,
    user_id: String,
    group_id: Option<String>,
) -> Result<(), String> {
    let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    match group_id {
        Some(g) => {
            groups.membership.insert(user_id, g);
        }
        None => {
            groups.membership.remove(&user_id);
        }
    }
    fs_utils::write_json(&state.path("groups.json"), &groups)?;
    Ok(())
}

// ---------------- 剩余积分 ----------------

/// 调用 TRAE API 计算剩余积分
/// 计算逻辑：遍历 user_entitlement_pack_list，仅对 quota.credits_limit 存在的包，
/// 剩余 = credits_limit - usage.credits_amount（usage 为空则已用=0），求和后四舍五入保留2位小数。
/// 同时返回最近过期的 expire_time（Unix 秒），用于积分过期感知调度。
/// 同时返回今日购买获得积分（start_time 在今日本地时间内且 charge_amount > 0）。
fn calc_remaining_credits(jwt: &str) -> Result<(f64, Option<i64>, f64), String> {
    let auth = if jwt.starts_with("Cloud-IDE-JWT ") {
        jwt.to_string()
    } else {
        format!("Cloud-IDE-JWT {}", jwt)
    };
    let resp = short_agent()
        .post("https://api.trae.cn/trae/api/v2/pay/ide_user_ent_usage")
        .set("authorization", &auth)
        .set("content-type", "application/json")
        .set("accept", "*/*")
        .send_json(ureq::json!({"require_usage": true, "req_source": 2}))
        .map_err(|e| format!("API 请求失败: {}", e))?;

    let body: serde_json::Value =
        resp.into_json().map_err(|e| format!("解析响应失败: {}", e))?;

    let packs = body
        .get("user_entitlement_pack_list")
        .and_then(|v| v.as_array())
        .ok_or("响应中缺少 user_entitlement_pack_list")?;

    let mut total: f64 = 0.0;
    let mut earliest_expire: Option<i64> = None;
    let mut today_non_checkin_earned: f64 = 0.0;

    // 使用固定 UTC+8 偏移，不依赖 chrono::Local（某些 Windows 环境下可能误判时区）
    let cst = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let now_ts = chrono::Utc::now().timestamp();

    // 今日北京时间范围 [00:00:00 +08:00, 23:59:59 +08:00]
    // start_time 来自 API 是 UTC Unix 时间戳，比较时需要按北京时间判定日期
    let today_start = chrono::Utc::now()
        .with_timezone(&cst)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(cst)
        .unwrap()
        .timestamp();
    let today_end = today_start + 86400;

    for pack in packs {
        // 仅对有 credits_limit 的包计入统计
        let credits_limit = pack
            .get("entitlement_base_info")
            .and_then(|e| e.get("quota"))
            .and_then(|q| q.get("credits_limit"))
            .and_then(|v| v.as_f64());
        if let Some(limit) = credits_limit {
            // usage 在 pack 顶层，不在 entitlement_base_info 内
            let used = pack
                .get("usage")
                .and_then(|u| u.get("credits_amount"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            total += (limit - used).max(0.0);

            // expire_time 也在 pack 顶层，取最早的（且未过期的）
            let expire = pack
                .get("expire_time")
                .and_then(|v| v.as_i64());
            if let Some(exp) = expire {
                if exp > now_ts {
                    earliest_expire = Some(earliest_expire.map_or(exp, |e| e.min(exp)));
                }
            }

            // 今日购买获得的积分：
            // start_time 在今日北京时间范围内，且 charge_amount > 0（实际付费购买）
            // 签到获得的 pack charge_amount=0，不会误判为购买积分
            let start_time = pack
                .get("entitlement_base_info")
                .and_then(|e| e.get("start_time"))
                .and_then(|v| v.as_i64());
            let charge_amount = pack
                .get("entitlement_base_info")
                .and_then(|e| e.get("charge_amount"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            // charge_amount > 0 表示付费购买（如会员连续包月），签到 pack charge_amount=0
            let is_purchased = charge_amount > 0;
            if let Some(st) = start_time {
                if st >= today_start && st < today_end && is_purchased {
                    today_non_checkin_earned += limit;
                }
            }
        }
    }

    // 四舍五入保留2位小数
    Ok((
        (total * 100.0).round() / 100.0,
        earliest_expire,
        (today_non_checkin_earned * 100.0).round() / 100.0,
    ))
}

/// 获取单个账号的剩余积分（实时请求 API）
#[tauri::command]
pub fn fetch_remaining_credits(state: State<AppState>, user_id: String) -> Result<f64, String> {
    let accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let account = accounts
        .accounts
        .iter()
        .find(|a| a.user_id.as_deref() == Some(user_id.as_str()))
        .ok_or("账号不存在")?;
    let jwt = &account.jwt;
    let (credits, expire_at, _non_checkin) = calc_remaining_credits(jwt)?;
    // 写入缓存
    let mut rc: RemainingCreditsFile = fs_utils::read_json(&state.path("remaining_credits.json"));
    rc.credits.insert(user_id.clone(), credits);
    if let Some(exp) = expire_at {
        rc.expire_times.insert(user_id, exp);
    }
    rc.updated_at = Some(fs_utils::now_iso());
    fs_utils::write_json(&state.path("remaining_credits.json"), &rc)?;
    Ok(credits)
}

/// 刷新所有账号的剩余积分（批量请求 API），返回成功数量。
/// 同时执行自动解冻：签到成功且有积分（credits > 0）且冷却类型非 SessionDead → 清除冷却。
#[tauri::command]
pub fn refresh_remaining_credits(state: State<AppState>) -> Result<usize, String> {
    let accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let mut rc: RemainingCreditsFile = fs_utils::read_json(&state.path("remaining_credits.json"));
    let mut cd: AccountCooldownsFile = fs_utils::read_json(&state.path("account_cooldowns.json"));
    let mut ok_count = 0usize;
    let mut thawed_count = 0usize;
    let mut total_non_checkin_earned: f64 = 0.0;
    for a in &accounts.accounts {
        let uid = a
            .user_id
            .clone()
            .or_else(|| jwt::parse(&a.jwt).user_id.clone())
            .unwrap_or_default();
        if uid.is_empty() {
            continue;
        }
        match calc_remaining_credits(&a.jwt) {
            Ok((credits, expire_at, non_checkin_earned)) => {
                rc.credits.insert(uid.clone(), credits);
                if let Some(exp) = expire_at {
                    rc.expire_times.insert(uid.clone(), exp);
                }
                total_non_checkin_earned += non_checkin_earned;
                ok_count += 1;
                // 自动解冻：有积分 + 冷却类型非 SessionDead → 清除
                if credits > 0.0 {
                    let thaw_type = cd.cooldowns.get(&uid).and_then(|e| {
                        if e.error_type != "SessionDead" && !e.error_type.is_empty() {
                            Some(e.error_type.clone())
                        } else {
                            None
                        }
                    });
                    if let Some(et) = thaw_type {
                        cd.cooldowns.remove(&uid);
                        thawed_count += 1;
                        crate::fs_utils::app_log(
                            &state.data_dir,
                            &format!("自动解冻 [{}]: 类型={} 积分={}", a.name, et, credits),
                        );
                    }
                }
            }
            Err(e) => {
                crate::fs_utils::app_log(
                    &state.data_dir,
                    &format!("获取剩余积分失败 [{}]: {}", a.name, e),
                );
            }
        }
    }
    rc.updated_at = Some(fs_utils::now_iso());
    fs_utils::write_json(&state.path("remaining_credits.json"), &rc)?;

    // 记录每日积分快照（total / earned / consumed）
    record_daily_snapshot(&state, &rc, total_non_checkin_earned);

    if thawed_count > 0 {
        cd.updated_at = Some(fs_utils::now_iso());
        fs_utils::write_json(&state.path("account_cooldowns.json"), &cd)?;
    }
    Ok(ok_count)
}

/// 记录每日积分快照（每天计算一次）：
/// - total = 所有账号剩余积分之和
/// - earned = 签到获得积分（credits_history.json delta 之和）+ 购买获得积分（API 查询 charge_amount > 0）
/// - consumed = |total - earned - 昨日total|（取绝对值）
fn record_daily_snapshot(state: &State<AppState>, rc: &RemainingCreditsFile, non_checkin_earned: f64) {
    let today = fs_utils::today_prefix(); // "YYYY-MM-DD"
    let total: f64 = rc.credits.values().sum();
    let total = (total * 100.0).round() / 100.0;

    let mut file: CreditsDailyFile = fs_utils::read_json(&state.path("credits_daily.json"));

    // earned = 签到获得积分（从 credits_history.json 汇总 delta）+ 非签到获得积分（API 查询）
    let credits_file: CreditsFile = fs_utils::read_json(&state.path("credits_history.json"));
    let checkin_earned: f64 = credits_file
        .records
        .iter()
        .filter(|r| r.date == today && r.user_id != "_daily_total")
        .map(|r| r.delta as f64)
        .sum();
    let earned = ((checkin_earned + non_checkin_earned) * 100.0).round() / 100.0;

    // 昨日积分总数：取 today 之前最近一条快照
    let yesterday_total = file
        .snapshots
        .iter()
        .filter(|s| s.date < today)
        .last()
        .map(|s| s.total)
        .unwrap_or(0.0);

    // consumed = |total - earned - yesterday_total|
    let consumed = (total - earned - yesterday_total).abs();
    let consumed = (consumed * 100.0).round() / 100.0;

    // 如果今天已有快照，更新全部字段（非首次记录也需刷新 earned/consumed）
    if let Some(existing) = file.snapshots.iter_mut().find(|s| s.date == today) {
        existing.total = total;
        existing.earned = earned;
        existing.consumed = consumed;
    } else {
        file.snapshots.push(CreditsDailySnapshot {
            date: today,
            total,
            earned,
            consumed,
        });
    }

    // 保留 90 天
    let cutoff = {
        let now = chrono::Utc::now();
        let cutoff_date = now - chrono::Duration::days(90);
        cutoff_date.format("%Y-%m-%d").to_string()
    };
    file.snapshots.retain(|s| s.date >= cutoff);

    let _ = fs_utils::write_json(&state.path("credits_daily.json"), &file);
}

/// 获取每日积分快照列表
#[tauri::command]
pub fn credits_daily_list(state: State<AppState>) -> Vec<CreditsDailySnapshot> {
    let file: CreditsDailyFile = fs_utils::read_json(&state.path("credits_daily.json"));
    file.snapshots
}

/// 手动清除指定账号的冷却状态
#[tauri::command]
pub fn cooldown_clear(state: State<AppState>, user_id: String) -> Result<(), String> {
    let mut cd: AccountCooldownsFile = fs_utils::read_json(&state.path("account_cooldowns.json"));
    if cd.cooldowns.remove(&user_id).is_some() {
        cd.updated_at = Some(fs_utils::now_iso());
        fs_utils::write_json(&state.path("account_cooldowns.json"), &cd)?;
    }
    Ok(())
}

/// 一键清除所有账号的冷却状态（用于所有账号被冷却导致 503 的场景）
/// 同时清除 JSON 文件中的持久化冷却记录和运行中 API 池的内存冷却状态
#[tauri::command]
pub fn cooldown_clear_all(
    state: State<AppState>,
    runtime: State<'_, std::sync::Mutex<Option<crate::commands::api_server::ApiServerRuntime>>>,
) -> Result<usize, String> {
    let mut cd: AccountCooldownsFile = fs_utils::read_json(&state.path("account_cooldowns.json"));
    let file_count = cd.cooldowns.len();
    if file_count > 0 {
        cd.cooldowns.clear();
        cd.updated_at = Some(fs_utils::now_iso());
        fs_utils::write_json(&state.path("account_cooldowns.json"), &cd)?;
    }

    // 同时清除运行中 API 池的内存冷却状态
    let mem_count = {
        let guard = runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(rt) => rt.shared.pool.clear_cooldowns(),
            None => 0,
        }
    };

    Ok(file_count.max(mem_count))
}

/// 使用 refresh_token 刷新 JWT（ExchangeToken）
/// 成功后原子写回新 accessToken + refresh_token，返回新 JWT
#[tauri::command]
pub fn refresh_jwt(state: State<AppState>, user_id: String) -> Result<String, String> {
    // 并发安全：持锁防止多个并发请求同时 ExchangeToken
    let _lock = state
        .jwt_refresh_lock
        .lock()
        .map_err(|_| "JWT 刷新锁获取失败")?;

    // Double-check：持锁后重新读取文件，防止其他线程已刷新
    let mut accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let account = accounts
        .accounts
        .iter()
        .find(|a| a.user_id.as_deref() == Some(user_id.as_str()))
        .ok_or("账号不存在")?;

    let refresh_token = account
        .refresh_token
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or("该账号无 refresh_token（浏览器提取的账号不支持自动刷新），请用「浏览器提取」重新获取 JWT")?;

    // 调用 ExchangeToken API
    let resp = short_agent()
        .post("https://api.trae.com.cn/cloudide/api/v3/trae/oauth/ExchangeToken")
        .set("content-type", "application/json")
        .set("accept", "*/*")
        .send_json(ureq::json!({
            "ClientID": "en1oxy7wnw8j9n",
            "RefreshToken": refresh_token,
            "ClientSecret": "-",
            "UserID": ""
        }))
        .map_err(|e| format!("ExchangeToken 请求失败: {}", e))?;

    let body: serde_json::Value =
        resp.into_json().map_err(|e| format!("解析响应失败: {}", e))?;

    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        return Err(format!("ExchangeToken 失败 (code={}): {}", code, msg));
    }

    let data = body
        .get("data")
        .ok_or("响应中缺少 data 字段")?;

    // 提取新 accessToken
    let new_access_token = data
        .get("access_token")
        .or_else(|| data.get("token"))
        .and_then(|v| v.as_str())
        .ok_or("响应中缺少 access_token")?;

    // 提取新 refresh_token（可能轮换）
    let new_refresh_token = data
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 验证新 accessToken 的 user_id 一致
    let new_jwt_full = if new_access_token.starts_with("Cloud-IDE-JWT ") {
        new_access_token.to_string()
    } else {
        format!("Cloud-IDE-JWT {}", new_access_token)
    };
    let new_info = jwt::parse(&new_jwt_full);
    if let Some(ref new_uid) = new_info.user_id {
        if new_uid != &user_id {
            return Err(format!(
                "刷新后 user_id 不匹配: 期望={}, 实际={}",
                user_id, new_uid
            ));
        }
    }

    // 原子写回
    let log_name = {
        let account = accounts
            .accounts
            .iter_mut()
            .find(|a| a.user_id.as_deref() == Some(user_id.as_str()))
            .ok_or("账号不存在")?;
        account.jwt = new_jwt_full.clone();
        if let Some(rt) = new_refresh_token {
            account.refresh_token = Some(rt);
        }
        account.updated_at = Some(fs_utils::now_iso());
        account.name.clone()
    };
    fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;

    crate::fs_utils::app_log(
        &state.data_dir,
        &format!(
            "JWT 自动刷新成功 [{}]: 新 exp={}",
            log_name,
            new_info
                .exp_hours
                .map(|h| format!("{:.1}h", h))
                .unwrap_or_else(|| "?".to_string())
        ),
    );

    Ok(new_jwt_full)
}

// ---------------- 内部工具 ----------------

/// 构建账号视图（聚合 JWT / 分组 / 设备 / 积分 / 今日签到 / 冷却状态）。
pub fn build_account_views(state: &State<AppState>) -> Vec<AccountView> {
    let accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    let groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
    let device_map: DeviceMap = fs_utils::read_json(&state.path("device_map.json"));
    let credits: CreditsFile = fs_utils::read_json(&state.path("credits_history.json"));
    let rc: RemainingCreditsFile = fs_utils::read_json(&state.path("remaining_credits.json"));
    let cd: AccountCooldownsFile = fs_utils::read_json(&state.path("account_cooldowns.json"));
    let summary: CheckinSummary = fs_utils::read_json(&state.path("checkin_summary.json"));
    let summary_today = summary
        .time
        .as_ref()
        .map(|t| t.starts_with(&fs_utils::today_prefix()))
        .unwrap_or(false);
    let checked_names: std::collections::HashSet<String> = if summary_today {
        summary
            .results
            .iter()
            .filter(|r| {
                let ok = r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let action = r.get("action").and_then(|v| v.as_str()).unwrap_or("");
                ok || (action != "fail" && !action.is_empty())
            })
            .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect()
    } else {
        Default::default()
    };

    let now_ts = chrono::Local::now().timestamp();
    let mut out = Vec::new();
    for a in &accounts.accounts {
        let uid = a
            .user_id
            .clone()
            .or_else(|| jwt::parse(&a.jwt).user_id.clone())
            .unwrap_or_default();
        let info = jwt::parse(&a.jwt);
        let group_id = groups.membership.get(&uid).cloned();
        // 取该账号最近日期的积分记录（credits_history.json 按日期追加，可能多条）；
        // 同日期取较大值，跨日期取较新日期，避免展示历史峰值而非当前余额。
        let credits_val = {
            let mut best: Option<(String, i64)> = None;
            for r in &credits.records {
                if r.user_id != uid {
                    continue;
                }
                match &best {
                    None => best = Some((r.date.clone(), r.credits)),
                    Some((d, c)) => {
                        if r.date > *d || (r.date == *d && r.credits > *c) {
                            best = Some((r.date.clone(), r.credits));
                        }
                    }
                }
            }
            best.map(|(_, c)| c)
        };
        let device_mask = device_map
            .get(&uid)
            .map(|d: &DeviceEntry| fs_utils::mask(&d.device_id));
        let checked = if summary_today {
            checked_names.contains(&a.name)
        } else {
            false
        };
        // 冷却状态：until > now 表示仍在冷却中（SessionDead 的 until=9999999999 始终 > now）
        let (cd_type, cd_until, cd_reason) = if let Some(entry) = cd.cooldowns.get(&uid) {
            if entry.until > now_ts && !entry.error_type.is_empty() {
                (
                    Some(entry.error_type.clone()),
                    Some(entry.until),
                    if entry.reason.is_empty() { None } else { Some(entry.reason.clone()) },
                )
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };
        let has_rt = a
            .refresh_token
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        // 自动刷新条件：有 refresh_token 且 JWT 24h 内过期或已过期
        let need_refresh = has_rt
            && info
                .exp_hours
                .map(|h| h <= 24.0)
                .unwrap_or(true);
        out.push(AccountView {
            user_id: uid.clone(),
            name: a.name.clone(),
            group_id,
            jwt: a.jwt.clone(),
            jwt_exp_hours: info.exp_hours,
            jwt_exp_timestamp: info.exp_timestamp,
            checked_today: Some(checked),
            credits: credits_val,
            remaining_credits: rc.credits.get(&uid).copied(),
            device_id_masked: device_mask,
            cooldown_type: cd_type,
            cooldown_until: cd_until,
            cooldown_reason: cd_reason,
            has_refresh_token: has_rt,
            jwt_auto_refresh: need_refresh,
            credits_expire_at: rc.expire_times.get(&uid).copied(),
        });
    }
    out
}

/// 根据 scope 解析目标 user_id 列表。
pub fn resolve_user_ids(
    state: &State<AppState>,
    scope: &str,
    selected: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let views = build_account_views(state);
    match scope {
        "all" => Ok(views.into_iter().map(|v| v.user_id).collect()),
        s if s.starts_with("group:") => {
            let gid = &s["group:".len()..];
            Ok(views
                .into_iter()
                .filter(|v| v.group_id.as_deref() == Some(gid))
                .map(|v| v.user_id)
                .collect())
        }
        "selected" => Ok(selected.unwrap_or_default()),
        _ => Err("未知的执行范围".into()),
    }
}
