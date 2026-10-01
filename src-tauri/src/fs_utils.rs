//! 文件读写工具：原子替换 + 容错加载 + 时间辅助。
use std::fs;
use std::io::Write;
use std::path::Path;

/// 读取 JSON，文件不存在或解析失败返回默认值。
pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    match fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => match serde_json::from_str(&s) {
            Ok(val) => val,
            Err(e) => {
                eprintln!("[fs_utils::read_json] 解析 {:?} 失败: {e}", path);
                T::default()
            }
        },
        _ => T::default(),
    }
}

/// 严格读取 JSON：文件不存在返回 Ok(default)；文件存在且非空时若解析失败返回 Err，绝不静默吞掉解析错误导致误删盘上数据。
pub fn read_json_strict<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    let s = fs::read_to_string(path).map_err(|e| format!("读取文件失败 {:?}: {e}", path))?;
    if s.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(&s).map_err(|e| format!("解析 JSON 文件失败 {:?}: {e}", path))
}

/// 原子写：先写临时文件再 rename，避免断电损坏。
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
        let buf = serde_json::to_vec_pretty(value).map_err(|e| format!("序列化失败: {e}"))?;
        f.write_all(&buf).map_err(|e| format!("写入失败: {e}"))?;
        f.flush().map_err(|e| format!("刷新失败: {e}"))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("替换文件失败: {e}"))?;
    Ok(())
}

/// 掩码：保留前4后4，中间用 … 代替。
pub fn mask(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 8 {
        return s.to_string();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{}…{}", head, tail)
}

pub fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn now_ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn today_prefix() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 按保留天数清理日志文件（proxy / checkin / switcher）。
/// 仅丢弃带 `[YYYY-MM-DD` 前缀且日期早于 cutoff 的行；无日期前缀的行（如部分外部脚本输出）一律保留。
/// 任何错误静默忽略——日志清理失败不应影响主流程。
pub fn trim_logs(data_dir: &Path, retention_days: u64) {
    if retention_days == 0 {
        return;
    }
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(retention_days as i64);
    let logs_dir = data_dir.join("logs");
    for name in ["proxy.log", "checkin.log", "switcher.log"] {
        let p = logs_dir.join(name);
        let content = match fs::read(&p) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Err(_) => continue,
        };
        let mut kept: Vec<&str> = Vec::new();
        for line in content.lines() {
            let date_part = line.strip_prefix('[').and_then(|s| s.get(..10));
            let drop = if let Some(d) = date_part {
                chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .map(|parsed| parsed < cutoff)
                    .unwrap_or(false)
            } else {
                false
            };
            if !drop {
                kept.push(line);
            }
        }
        let new_content = kept.join("\n");
        if new_content != content {
            let _ = fs::write(&p, new_content);
        }
    }
}

/// 追加一行到 data_dir/logs/app.log，用于托盘/通知等关键路径排查。
pub fn app_log(data_dir: &Path, msg: &str) {
    let log_path = data_dir.join("logs").join("app.log");
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(f, "[{}] {}", now_ts(), msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_json_strict() {
        let dir = std::env::temp_dir().join(format!("test_json_strict_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.json");

        // 1. 文件不存在返回 Ok(default)
        let _ = std::fs::remove_file(&path);
        let val: serde_json::Value = read_json_strict(&path).unwrap();
        assert_eq!(val, serde_json::Value::Null);

        // 2. 文件为空返回 Ok(default)
        std::fs::write(&path, "").unwrap();
        let val: serde_json::Value = read_json_strict(&path).unwrap();
        assert_eq!(val, serde_json::Value::Null);

        // 3. 文件有损坏数据返回 Err（绝不静默回退 default 导致数据覆盖）
        std::fs::write(&path, "{ invalid json").unwrap();
        assert!(read_json_strict::<serde_json::Value>(&path).is_err());

        // 4. 正确数据正常解析
        std::fs::write(&path, r#"{"hello":"world"}"#).unwrap();
        let val: serde_json::Value = read_json_strict(&path).unwrap();
        assert_eq!(val["hello"], "world");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
