//! 浏览器一键提取 JWT 登录：通过 CDP 驱动系统 Edge/Chrome，
//! 拦截 trae API 请求的 Authorization 头完成账号保存。
//! 设计文档：docs/superpowers/specs/2026-09-03-browser-extract-jwt-design.md

/// 常用浏览器可执行文件候选路径（Edge 优先于 Chrome，内核一致且更普及）
const BROWSER_CANDIDATES: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// 调试端口扫描范围（避开常用 9222，减少与用户自己开的调试端口冲突）
const DEBUG_PORT_RANGE: std::ops::RangeInclusive<u16> = 9333..=9433;

/// 归一化 token：兼容 `Cloud-IDE-JWT x` / `Bearer x` / 裸 token，
/// 统一为 `Cloud-IDE-JWT x` 前缀格式（与 oauth.rs / accounts.rs 保存格式一致）
pub(crate) fn normalize_token(raw: &str) -> String {
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Cloud-IDE-JWT ")
        .or_else(|| trimmed.strip_prefix("Bearer "))
        .unwrap_or(trimmed)
        .trim();
    format!("Cloud-IDE-JWT {}", token)
}

/// 是否为 trae API 请求（JWT 出现在这些请求的 Authorization 头中）
pub(crate) fn is_trae_api_url(url: &str) -> bool {
    url.contains("api.trae.com.cn")
}

/// 从 CDP 请求头 JSON 中提取 Authorization 值（头名大小写不敏感）。
/// 入参为 CDP `Network.Request.headers`（Headers newtype 的 inner，
/// 形如 {"Authorization": "Bearer x", ...}；非对象时返回 None）
pub(crate) fn auth_header(headers: &serde_json::Value) -> Option<String> {
    let obj = headers.as_object()?;
    for (k, v) in obj {
        if k.eq_ignore_ascii_case("authorization") {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// 从候选路径中选出第一个存在的文件
pub(crate) fn pick_existing(paths: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    paths.iter().find(|p| p.is_file()).cloned()
}

/// 浏览器发现：配置了 browser_path 就只用它（配错直接报错，不静默回退）；
/// 未配置则按内置候选顺序探测 Edge → Chrome
pub(crate) fn find_browser(custom: Option<&str>) -> Option<std::path::PathBuf> {
    match custom {
        Some(c) if !c.trim().is_empty() => {
            let p = std::path::PathBuf::from(c.trim());
            if p.is_file() { Some(p) } else { None }
        }
        _ => pick_existing(
            &BROWSER_CANDIDATES
                .iter()
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>(),
        ),
    }
}

/// 在调试端口范围内找一个当前空闲的端口（存在极小竞争窗口，CDP 连接失败会走报错路径）
#[allow(dead_code)] // Task 5 使用
fn find_free_port() -> Option<u16> {
    for port in DEBUG_PORT_RANGE {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Some(port);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_token_bearer() {
        assert_eq!(normalize_token("Bearer abc123"), "Cloud-IDE-JWT abc123");
    }

    #[test]
    fn normalize_token_cloud_ide_prefix_kept() {
        assert_eq!(
            normalize_token("Cloud-IDE-JWT abc123"),
            "Cloud-IDE-JWT abc123"
        );
    }

    #[test]
    fn normalize_token_bare() {
        assert_eq!(normalize_token("abc123"), "Cloud-IDE-JWT abc123");
    }

    #[test]
    fn normalize_token_trims_whitespace() {
        assert_eq!(normalize_token("  Bearer   abc \n"), "Cloud-IDE-JWT abc");
    }

    #[test]
    fn tra_api_url_matches() {
        assert!(is_trae_api_url(
            "https://api.trae.com.cn/cloudide/api/v3/trae/GetUserInfo"
        ));
    }

    #[test]
    fn tra_api_url_rejects_site() {
        assert!(!is_trae_api_url("https://www.trae.cn/"));
    }

    #[test]
    fn auth_header_found() {
        assert_eq!(
            auth_header(&json!({"Authorization": "Bearer xyz"})),
            Some("Bearer xyz".to_string())
        );
    }

    #[test]
    fn auth_header_case_insensitive() {
        assert_eq!(
            auth_header(&json!({"authorization": "Bearer xyz"})),
            Some("Bearer xyz".to_string())
        );
    }

    #[test]
    fn auth_header_absent() {
        assert_eq!(
            auth_header(&json!({"Content-Type": "application/json"})),
            None
        );
    }

    #[test]
    fn auth_header_non_object_returns_none() {
        assert_eq!(auth_header(&json!("not an object")), None);
        assert_eq!(auth_header(&json!(null)), None);
    }

    #[test]
    fn auth_header_whitespace_value_returns_none() {
        assert_eq!(auth_header(&json!({"Authorization": "   "})), None);
    }

    #[test]
    fn pick_existing_finds_first() {
        let tmp = std::env::temp_dir();
        let a = tmp.join("be_test_missing_a.exe");
        let b = tmp.join(format!("be_test_exists_{}.exe", std::process::id()));
        let _ = std::fs::remove_file(&b);
        std::fs::write(&b, b"x").unwrap();
        assert_eq!(pick_existing(&[a, b.clone()]), Some(b.clone()));
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn pick_existing_none_when_missing() {
        let tmp = std::env::temp_dir();
        let a = tmp.join("be_test_missing_b1.exe");
        let b = tmp.join("be_test_missing_b2.exe");
        assert_eq!(pick_existing(&[a, b]), None);
    }

    #[test]
    fn find_browser_custom_valid() {
        let tmp = std::env::temp_dir();
        let p = tmp.join(format!("be_test_browser_{}.exe", std::process::id()));
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(find_browser(Some(p.to_str().unwrap())), Some(p.clone()));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn find_browser_custom_invalid_returns_none() {
        assert_eq!(find_browser(Some(r"C:\nonexistent\browser.exe")), None);
    }
}
