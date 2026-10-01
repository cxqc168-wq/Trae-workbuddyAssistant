//! WorkBuddy 会话自动复制队列：切号后把源账号会话复制到目标账号。
//!
//! 全局单条串行队列，保证 A→B→C 快速连切时后一个任务能看到前一个任务创建的行。
//! 每个会话之间让出执行权，避免大量 I/O 饿死 UI。

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

use super::session_db::{SessionRecord, WbSessionDb};
use super::session_lineage;

#[derive(Serialize, Clone)]
pub struct SessionJob {
    pub id: String,
    pub source_uid: String,
    pub target_uid: String,
    pub status: String, // "queued" | "running" | "done" | "partial" | "failed"
    pub total: usize,
    pub processed: usize,
    pub copied: usize,
    pub skipped: usize,
    pub partial: usize,
    pub failed: usize,
}

// 全局串行锁：保证同一时刻只有一个复制任务在执行
static SERIAL_LOCK: Mutex<()> = Mutex::new(());

// 最近完成的 job（供前端查询）
static LAST_JOB: Mutex<Option<SessionJob>> = Mutex::new(None);

/// 启动一个自动复制任务（异步，立即返回 job id）。
pub fn start_job(
    app: AppHandle,
    source_uid: String,
    target_uid: String,
) -> Result<SessionJob, String> {
    if source_uid == target_uid {
        return Err("源账号与目标账号相同".into());
    }
    let job_id = uuid::Uuid::new_v4().to_string();
    let job = SessionJob {
        id: job_id.clone(),
        source_uid: source_uid.clone(),
        target_uid: target_uid.clone(),
        status: "queued".into(),
        total: 0,
        processed: 0,
        copied: 0,
        skipped: 0,
        partial: 0,
        failed: 0,
    };

    let app_clone = app.clone();
    let job_clone = job.clone();

    // spawn 后台任务
    std::thread::spawn(move || {
        // 获取串行锁，保证任务串行执行
        let _guard = SERIAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut job = job_clone;
        job.status = "running".into();

        // 1. 打开数据库，列出源账号会话
        let db = match WbSessionDb::open() {
            Ok(db) => db,
            Err(e) => {
                job.status = "failed".into();
                job.failed = 1;
                emit_job(&app_clone, &job, Some(&format!("打开会话库失败: {}", e)));
                save_last_job(&job);
                return;
            }
        };

        let sessions: Vec<SessionRecord> = match db.list_sessions(&source_uid) {
            Ok(s) => s,
            Err(e) => {
                job.status = "failed".into();
                job.failed = 1;
                emit_job(&app_clone, &job, Some(&format!("查询源会话失败: {}", e)));
                save_last_job(&job);
                return;
            }
        };
        job.total = sessions.len();
        emit_job(&app_clone, &job, None);

        if job.total == 0 {
            job.status = "done".into();
            emit_job(&app_clone, &job, None);
            save_last_job(&job);
            return;
        }

        // 2. 逐会话复制
        for src in &sessions {
            // 让出一点时间，避免饿死 UI 事件循环
            std::thread::sleep(std::time::Duration::from_millis(10));

            // 检查目标账号是否已有该会话的 lineage 副本
            let lineage_id = session_lineage::find_lineage(&source_uid, &src.id);
            let target_existing = lineage_id
                .as_ref()
                .and_then(|lid| session_lineage::get_mapping(lid, &target_uid));

            let result = if let Some(mapping) = target_existing {
                // 目标已有副本：更新文件（overwrite），不新建
                match super::session_files::copy_remap(&src.id, &mapping.target_id, true) {
                    Ok(report) => {
                        let status = if report.failed > 0 { "partial" } else { "skipped" };
                        (mapping.target_id, status.to_string(), report.failed)
                    }
                    Err(e) => (String::new(), "failed".to_string(), 0),
                }
            } else {
                // 新建副本
                let lid = lineage_id
                    .clone()
                    .or_else(|| session_lineage::ensure_lineage(&source_uid, &src.id).ok());
                match session_lineage::copy_session(&db, src, &target_uid, lid.as_deref()) {
                    Ok(r) => r,
                    Err(_) => (String::new(), "failed".to_string(), 0),
                }
            };

            match result.1.as_str() {
                "copied" => job.copied += 1,
                "skipped" => job.skipped += 1,
                "partial" => job.partial += 1,
                _ => job.failed += 1,
            }
            if result.2 > 0 {
                job.failed += result.2 as usize;
            }
            job.processed += 1;
            emit_job(&app_clone, &job, None);
        }

        job.status = if job.failed > 0 || job.partial > 0 {
            "partial".to_string()
        } else {
            "done".to_string()
        };
        emit_job(&app_clone, &job, None);
        save_last_job(&job);
    });

    save_last_job(&job);
    Ok(job)
}

fn emit_job(app: &AppHandle, job: &SessionJob, error: Option<&str>) {
    let mut payload = serde_json::to_value(job).unwrap_or(serde_json::Value::Null);
    if let Some(e) = error {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("error".into(), serde_json::Value::String(e.to_string()));
        }
    }
    let _ = app.emit("wb-session-job", &payload);
}

fn save_last_job(job: &SessionJob) {
    if let Ok(mut g) = LAST_JOB.lock() {
        *g = Some(job.clone());
    }
}

/// 查询最近一次 job 状态。
pub fn last_job() -> Option<SessionJob> {
    LAST_JOB.lock().ok().and_then(|g| g.clone())
}

/// 手动复制指定会话到目标账号（单条，不入队列）。
pub fn copy_single_session(
    source_uid: &str,
    session_id: &str,
    target_uid: &str,
) -> Result<(String, String, u64), String> {
    let db = WbSessionDb::open()?;
    let src = db
        .get_session(session_id)?
        .ok_or("源会话不存在")?;
    if src.user_id != source_uid {
        return Err("源会话不属于指定账号".into());
    }
    let lineage_id = session_lineage::find_lineage(source_uid, session_id)
        .or_else(|| session_lineage::ensure_lineage(source_uid, session_id).ok());
    session_lineage::copy_session(&db, &src, target_uid, lineage_id.as_deref())
}
