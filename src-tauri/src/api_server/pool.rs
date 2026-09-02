use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Sha256, Digest};

use crate::models::{CooldownEntry, DeviceMap, PoolStatus};

use super::ErrKind;

/// 池中单个账号的运行时状态
pub struct PoolEntry {
    pub uid: String,
    pub name: String,
    pub jwt: String,
    pub credits: Option<f64>,
    pub credits_expire_at: Option<i64>,
    pub disabled: bool,
    pub err_count: i32,
    /// 冷却截止时间（Unix 秒），0 表示无冷却
    pub until: i64,
    pub reason: String,
    pub device_id: String,
    pub machine_id: String,
}

impl PoolEntry {
    fn healthy(&self, now_ts: i64) -> bool {
        if self.disabled {
            return false;
        }
        if self.until > 0 && now_ts < self.until {
            return false;
        }
        true
    }
}

/// 账号池：内存索引 + 冷却/禁用状态机
pub struct ApiPool {
    entries: Mutex<HashMap<String, PoolEntry>>,
}

/// 安全获取 Mutex 锁：若锁被毒化（panic 导致），仍恢复内部数据继续运行
fn safe_lock<'a, T>(m: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ApiPool {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 从已有账号文件同步池：只加入 enabled_uids 中的账号
    pub fn sync_from_accounts(
        &self,
        accounts: &[crate::models::RawAccount],
        enabled_uids: &[String],
        cooldowns: &HashMap<String, CooldownEntry>,
        remaining_credits: &HashMap<String, f64>,
        expire_times: &HashMap<String, i64>,
        device_map: &DeviceMap,
    ) {
        let mut entries = safe_lock(&self.entries);
        entries.clear();
        let enabled: HashSet<&str> = enabled_uids.iter().map(|s| s.as_str()).collect();
        for a in accounts {
            if let Some(uid) = &a.user_id {
                if !enabled.contains(uid.as_str()) {
                    continue;
                }
                let cd = cooldowns.get(uid).cloned().unwrap_or_default();
                let disabled = cd.error_type == "SessionDead";
                let (device_id, machine_id) = device_map
                    .get(uid)
                    .map(|d| (d.device_id.clone(), seeded_hex(64, uid, "mach")))
                    .unwrap_or_else(|| (String::new(), seeded_hex(64, uid, "mach")));
                let jwt_raw = a.jwt.clone();
                let jwt_clean = jwt_raw
                    .strip_prefix("Cloud-IDE-JWT ")
                    .unwrap_or(&jwt_raw)
                    .trim()
                    .to_string();
                entries.insert(
                    uid.clone(),
                    PoolEntry {
                        uid: uid.clone(),
                        name: a.name.clone(),
                        jwt: jwt_clean,
                        credits: remaining_credits.get(uid).copied(),
                        credits_expire_at: expire_times.get(uid).copied(),
                        disabled,
                        err_count: cd.error_count,
                        until: cd.until,
                        reason: cd.reason,
                        device_id,
                        machine_id,
                    },
                );
            }
        }
    }

    /// 挑选 healthy 账号中积分过期时间最近者；跳过 tried
    /// llm_utils_chat 消耗 IDE 积分(product_id 208)
    /// 零积分账号会被跳过，避免无效请求
    pub fn pick_excluding(&self, tried: &HashSet<String>) -> Option<PickedAccount> {
        let entries = safe_lock(&self.entries);
        let now = now_ts();
        let mut best: Option<&PoolEntry> = None;
        for (uid, e) in entries.iter() {
            if tried.contains(uid) || !e.healthy(now) {
                continue;
            }
            // 跳过积分已过期的（expire_time=0 视为无过期时间，不跳过）
            if let Some(exp) = e.credits_expire_at {
                if exp > 0 && exp < now {
                    continue;
                }
            }
            // 跳过零积分账号（IDE 积分耗尽，llm_utils_chat 无法使用）
            if let Some(c) = e.credits {
                if c <= 0.0 {
                    continue;
                }
            }
            match best {
                None => best = Some(e),
                Some(b) => {
                    let be = b.credits_expire_at.is_some();
                    let ee = e.credits_expire_at.is_some();
                    if ee && !be {
                        best = Some(e);
                    } else if ee && be {
                        if e.credits_expire_at < b.credits_expire_at {
                            best = Some(e);
                        } else if e.credits_expire_at == b.credits_expire_at {
                            if e.credits.unwrap_or(0.0) > b.credits.unwrap_or(0.0) {
                                best = Some(e);
                            }
                        }
                    }
                }
            }
        }
        best.map(|e| PickedAccount {
            uid: e.uid.clone(),
            jwt: e.jwt.clone(),
            device_id: e.device_id.clone(),
            machine_id: e.machine_id.clone(),
        })
    }

    /// 记录错误并冷却
    pub fn note_error(&self, uid: &str, kind: ErrKind) {
        let dur = kind.cooldown_duration();
        let mut entries = safe_lock(&self.entries);
        if let Some(e) = entries.get_mut(uid) {
            if kind == ErrKind::SessionDead {
                e.disabled = true;
            } else if kind == ErrKind::PlanLimit || kind == ErrKind::SoftRate || kind == ErrKind::NotFound {
                e.until = now_ts() + dur.as_secs() as i64;
                e.reason = kind.as_str().to_string();
                e.err_count = 0;
            } else {
                e.err_count += 1;
                if e.err_count >= 3 {
                    e.until = now_ts() + dur.as_secs() as i64;
                    e.reason = "consecutive_errors".to_string();
                    e.err_count = 0;
                }
            }
        }
    }

    /// 记录成功
    pub fn note_success(&self, uid: &str) {
        let mut entries = safe_lock(&self.entries);
        if let Some(e) = entries.get_mut(uid) {
            e.err_count = 0;
        }
    }

    /// 清除所有账号的内存冷却状态（不影响 disabled/SessionDead）
    pub fn clear_cooldowns(&self) -> usize {
        let mut entries = safe_lock(&self.entries);
        let now = now_ts();
        let count = entries
            .values()
            .filter(|e| e.until > 0 && now < e.until)
            .count();
        for e in entries.values_mut() {
            e.until = 0;
            e.reason.clear();
            e.err_count = 0;
        }
        count
    }

    /// 返回池状态列表
    pub fn status_list(&self) -> Vec<PoolStatus> {
        let entries = safe_lock(&self.entries);
        let now = now_ts();
        let mut out: Vec<PoolStatus> = entries
            .values()
            .map(|e| PoolStatus {
                uid: e.uid.clone(),
                name: e.name.clone(),
                credits: e.credits,
                credits_expire_at: e.credits_expire_at,
                cooling: e.until > 0 && now < e.until,
                cooldown_until: if e.until > 0 { Some(e.until) } else { None },
                cooldown_reason: if e.reason.is_empty() { None } else { Some(e.reason.clone()) },
                disabled: e.disabled,
                err_count: e.err_count,
            })
            .collect();
        out.sort_by(|a, b| a.uid.cmp(&b.uid));
        out
    }

    pub fn count(&self) -> usize {
        safe_lock(&self.entries).len()
    }

    /// 诊断：返回所有账号被过滤的原因（用于 "no healthy account" 排查）
    pub fn diagnose(&self) -> Vec<PoolDiagnosis> {
        let entries = safe_lock(&self.entries);
        let now = now_ts();
        entries
            .values()
            .map(|e| {
                let reason = if e.disabled {
                    "disabled(SessionDead)".to_string()
                } else if e.until > 0 && now < e.until {
                    format!("cooldown(until={} remaining={}s)", e.until, e.until - now)
                } else if let Some(exp) = e.credits_expire_at {
                    if exp > 0 && exp < now {
                        "credits_expired".to_string()
                    } else if exp > 0 {
                        if let Some(c) = e.credits {
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
                    }
                } else {
                    "healthy(no_expiry_info)".to_string()
                };
                PoolDiagnosis {
                    uid: e.uid.clone(),
                    name: e.name.clone(),
                    disabled: e.disabled,
                    until: e.until,
                    credits: e.credits,
                    credits_expire_at: e.credits_expire_at,
                    reason,
                }
            })
            .collect()
    }
}

/// 账号池诊断信息
pub struct PoolDiagnosis {
    pub uid: String,
    pub name: String,
    pub disabled: bool,
    pub until: i64,
    pub credits: Option<f64>,
    pub credits_expire_at: Option<i64>,
    pub reason: String,
}

pub struct PickedAccount {
    pub uid: String,
    pub jwt: String,
    pub device_id: String,
    pub machine_id: String,
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 确定性派生 hex 字符串（与 device_proxy.py 的 _seeded_stream 算法一致）
/// 用于从 uid 生成 machine_id，保证同一账号始终得到同一设备标识
fn seeded_hex(n: usize, seed: &str, salt: &str) -> String {
    let data = format!("{}:{}", salt, seed);
    let mut out = Vec::new();
    let mut i: u32 = 0;
    while out.len() < (n + 1) / 2 {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        hasher.update(i.to_be_bytes());
        out.extend_from_slice(&hasher.finalize());
        i += 1;
    }
    out.truncate((n + 1) / 2);
    out.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        .chars().take(n).collect()
}
