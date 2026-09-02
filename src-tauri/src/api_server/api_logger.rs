use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local};

/// API 请求日志记录器：按日期分文件，直接存储在 logs/ 目录下（文件名 api_YYYY-MM-DD.log）
pub struct ApiLogger {
    dir: PathBuf,
    writer: Mutex<Option<std::fs::File>>,
    max_size: u64,
}

impl ApiLogger {
    pub fn new(dir: PathBuf) -> Self {
        fs::create_dir_all(&dir).ok();
        Self {
            dir,
            writer: Mutex::new(None),
            max_size: 50 * 1024 * 1024, // 50MB 滚动
        }
    }

    fn today_filename() -> String {
        // 使用 chrono 本地时间获取精确日期
        let now = Local::now();
        format!("api_{:04}-{:02}-{:02}.log", now.year(), now.month(), now.day())
    }

    /// 获取或创建今日日志文件句柄
    pub fn get_writer(&self) -> Option<std::fs::File> {
        let today = Self::today_filename();
        let path = self.dir.join(&today);

        // 检查当前 writer 是否仍然有效（文件名相同且未超大小）
        let mut guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref f) = *guard {
            if let Ok(meta) = f.metadata() {
                if meta.len() < self.max_size {
                    // 当前文件可用，返回 clone
                    if let Ok(clone) = f.try_clone() {
                        return Some(clone);
                    }
                }
            }
        }

        // 需要新建文件
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        *guard = Some(file.try_clone().ok()?);
        Some(file)
    }

    /// 记录一条 API 请求日志
    pub fn log_request(
        &self,
        method: &str,
        path: &str,
        model: &str,
        stream: bool,
        status: u16,
        uid: &str,
        duration_ms: u64,
        error: Option<&str>,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let local_ts = now + 8 * 3600;
        let h = (local_ts % 86400) / 3600;
        let m = (local_ts % 3600) / 60;
        let s = local_ts % 60;

        let uid_short = &uid[..uid.len().min(12)];
        let err_part = match error {
            Some(e) => format!(" error={}", e),
            None => String::new(),
        };

        let line = format!(
            "[{:02}:{:02}:{:02}] {} {} model={} stream={} status={} uid={} {}ms{}\n",
            h, m, s,
            method, path, model, stream, status, uid_short, duration_ms, err_part,
        );

        if let Some(mut f) = self.get_writer() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// 记录 Debug 级别的完整请求/响应日志
    pub fn log_debug(
        &self,
        uid: &str,
        req_body: &[u8],
        resp_body: Option<&[u8]>,
        status: u16,
        error: Option<&str>,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let local_ts = now + 8 * 3600;
        let h = (local_ts % 86400) / 3600;
        let m = (local_ts % 3600) / 60;
        let s = local_ts % 60;

        let uid_short = &uid[..uid.len().min(12)];
        let mut lines = Vec::new();
        lines.push(format!(
            "[{:02}:{:02}:{:02}] [DEBUG] uid={} status={}",
            h, m, s, uid_short, status,
        ));

        // 请求体（截取前 4KB 防止日志爆炸）
        let req_preview = if req_body.len() > 4096 {
            &req_body[..4096]
        } else {
            req_body
        };
        let req_str = String::from_utf8_lossy(req_preview);
        lines.push(format!("--- Request Body ({} bytes, preview {}B) ---", req_body.len(), req_preview.len()));
        lines.push(req_str.to_string());

        // 响应体（截取前 8KB）
        if let Some(resp) = resp_body {
            let resp_preview = if resp.len() > 8192 {
                &resp[..8192]
            } else {
                resp
            };
            let resp_str = String::from_utf8_lossy(resp_preview);
            lines.push(format!("--- Response Body ({} bytes, preview {}B) ---", resp.len(), resp_preview.len()));
            lines.push(resp_str.to_string());
        }

        if let Some(e) = error {
            lines.push(format!("--- Error: {} ---", e));
        }

        lines.push(String::new()); // 空行分隔

        if let Some(mut f) = self.get_writer() {
            let data = lines.join("\n");
            let _ = f.write_all(data.as_bytes());
        }
    }

    /// 读取指定日期的日志文件内容（按时间倒序排列）
    pub fn read_log(&self, date: &str) -> Option<String> {
        // date 格式: "2026-08-14"
        let path = self.dir.join(format!("api_{}.log", date));
        match fs::read(&path) {
            Ok(bytes) => {
                let content = String::from_utf8_lossy(&bytes).to_string();
                Some(reverse_log_blocks(&content))
            }
            Err(_) => None,
        }
    }

    /// 按时间段和关键字搜索日志
    ///
    /// - `date`: 日期，格式 "2026-08-14"
    /// - `start_time`: 起始时间，格式 "HH:MM:SS" 或 "HH:MM"，为空则不限
    /// - `end_time`: 结束时间，同上
    /// - `keyword`: 关键字（不区分大小写），为空则不限
    ///
    /// 返回过滤后的日志文本。普通日志条目按行过滤；
    /// Debug 块（多行）只要任一行命中关键字则整块保留。
    pub fn search_log(
        &self,
        date: &str,
        start_time: &str,
        end_time: &str,
        keyword: &str,
    ) -> Option<String> {
        let content = self.read_log(date)?;

        // 预处理时间边界（补全为 HH:MM:SS 格式）
        let start = normalize_time(start_time);
        let end = normalize_time(end_time);
        let kw_lower = keyword.to_lowercase();
        let has_kw = !kw_lower.is_empty();
        let has_time = !start.is_empty() || !end.is_empty();

        if !has_kw && !has_time {
            return Some(content);
        }

        let mut result: Vec<String> = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // 检测 Debug 块：以 [HH:MM:SS] [DEBUG] 开头
            if line.contains("[DEBUG]") && line.starts_with('[') {
                // 收集整个 Debug 块（直到下一个 [ 开头的行或空行后）
                let mut block: Vec<&str> = vec![line];
                let block_time = extract_time(line);
                let mut j = i + 1;
                while j < lines.len() {
                    let next = lines[j];
                    // 遇到新日志条目（以 [HH:MM:SS] 开头且非 --- 开头）则停止
                    if next.starts_with('[') && extract_time(next).is_some() {
                        break;
                    }
                    block.push(next);
                    j += 1;
                }

                // 时间过滤：用块首行的时间
                let time_ok = if has_time {
                    is_time_in_range(&block_time, &start, &end)
                } else {
                    true
                };

                // 关键字过滤：块中任一行命中即可
                let kw_ok = if has_kw {
                    block.iter().any(|l| l.to_lowercase().contains(&kw_lower))
                } else {
                    true
                };

                if time_ok && kw_ok {
                    result.push(block.join("\n"));
                }

                i = j;
            } else {
                // 普通单行日志
                let time = extract_time(line);

                let time_ok = if has_time {
                    is_time_in_range(&time, &start, &end)
                } else {
                    true
                };

                let kw_ok = if has_kw {
                    line.to_lowercase().contains(&kw_lower)
                } else {
                    true
                };

                if time_ok && kw_ok {
                    result.push(line.to_string());
                }

                i += 1;
            }
        }

        if result.is_empty() {
            Some("（无匹配的日志条目）".to_string())
        } else {
            // 倒序排列（最新的在最前面）
            result.reverse();
            Some(result.join("\n"))
        }
    }

    /// 列出所有可用日志日期（最近 N 天）
    pub fn list_dates(&self, max: usize) -> Vec<String> {
        let mut dates: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // 匹配 api_YYYY-MM-DD.log
                if name.starts_with("api_") && name.ends_with(".log") {
                    let date = &name[4..name.len() - 4];
                    dates.push(date.to_string());
                }
            }
        }
        dates.sort();
        dates.reverse();
        dates.truncate(max);
        dates
    }
}

// ==================== 辅助函数 ====================

/// 从日志行中提取时间部分 "HH:MM:SS"
/// 日志行格式: `[HH:MM:SS] ...` 或 `[HH:MM:SS] [DEBUG] ...`
fn extract_time(line: &str) -> Option<String> {
    if !line.starts_with('[') {
        return None;
    }
    // 提取第一个 ] 之前的内容（不含 [ ）
    let close = line.find(']')?;
    let inner = &line[1..close];
    // 验证格式 HH:MM:SS
    let parts: Vec<&str> = inner.split(':').collect();
    if parts.len() == 3 {
        let h = parts[0].parse::<u32>().ok()?;
        let m = parts[1].parse::<u32>().ok()?;
        let s = parts[2].parse::<u32>().ok()?;
        if h < 24 && m < 60 && s < 60 {
            return Some(format!("{:02}:{:02}:{:02}", h, m, s));
        }
    }
    // 也支持 HH:MM 格式（不含秒）
    if parts.len() == 2 {
        let h = parts[0].parse::<u32>().ok()?;
        let m = parts[1].parse::<u32>().ok()?;
        if h < 24 && m < 60 {
            return Some(format!("{:02}:{:02}:00", h, m));
        }
    }
    None
}

/// 将时间字符串标准化为 "HH:MM:SS" 格式
/// 输入可以是 "HH:MM" 或 "HH:MM:SS"
fn normalize_time(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = t.split(':').collect();
    match parts.len() {
        2 => {
            let h = parts[0].parse::<u32>().unwrap_or(0);
            let m = parts[1].parse::<u32>().unwrap_or(0);
            format!("{:02}:{:02}:00", h, m)
        }
        3 => {
            let h = parts[0].parse::<u32>().unwrap_or(0);
            let m = parts[1].parse::<u32>().unwrap_or(0);
            let s = parts[2].parse::<u32>().unwrap_or(0);
            format!("{:02}:{:02}:{:02}", h, m, s)
        }
        _ => String::new(),
    }
}

/// 判断时间是否在 [start, end] 范围内
/// start/end 为 "HH:MM:SS" 格式，为空表示不限
fn is_time_in_range(time: &Option<String>, start: &str, end: &str) -> bool {
    let Some(t) = time else {
        // 无法解析时间的行（如空行、分隔线），放行
        return true;
    };
    if !start.is_empty() && t.as_str() < start {
        return false;
    }
    if !end.is_empty() && t.as_str() > end {
        return false;
    }
    true
}

/// 将日志内容按块倒序排列（最新的条目在最前面）
/// 普通单行日志以一个块处理，Debug 多行块（以 [DEBUG] 开头的条目及其后续行）作为整体处理
fn reverse_log_blocks(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Debug 块：以 [ 开头且包含 [DEBUG]
        if line.contains("[DEBUG]") && line.starts_with('[') {
            let mut block: Vec<&str> = vec![line];
            let mut j = i + 1;
            while j < lines.len() {
                let next = lines[j];
                // 遇到新日志条目（以 [HH:MM:SS] 开头）则停止
                if next.starts_with('[') && extract_time(next).is_some() {
                    break;
                }
                block.push(next);
                j += 1;
            }
            blocks.push(block.join("\n"));
            i = j;
        } else if line.starts_with('[') && extract_time(line).is_some() {
            // 普通单行日志
            blocks.push(line.to_string());
            i += 1;
        } else {
            // 无法解析的行（空行等），合并到前一个块
            if let Some(last) = blocks.last_mut() {
                last.push('\n');
                last.push_str(line);
            } else {
                blocks.push(line.to_string());
            }
            i += 1;
        }
    }

    blocks.reverse();
    blocks.join("\n")
}
