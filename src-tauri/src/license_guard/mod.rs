//! license-guard 授权防护（Rust 移植版，协议与 license-guard Python 客户端完全兼容）。
//!
//! 协议：
//! - 激活：POST {SERVER_URL}/api/activate {code, machine_id, nonce}
//!   -> {"payload_b64": canonical JSON 的 Base64, "signature_b64": RSA-SHA256 签名的 Base64}
//! - 凭证：%USERPROFILE%\.license_guard\license.dat = {payload_b64, signature_b64, last_seen}
//! - 校验：内置公钥验签 -> machine_id 比对 -> expires_at 检查 -> last_seen 时钟回拨检查
//!
//! 安全属性与 Python 版一致：
//! - 客户端只内置公钥，无法离线伪造凭证（签发需要只在服务器上的私钥）
//! - 凭证绑定机器指纹，复制到其他电脑无效
//! - 每次校验通过更新 last_seen，改系统时间无法无限续期

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// 验证服务器地址；可用环境变量 LICENSE_GUARD_SERVER_URL 覆盖（仅改地址，
/// 验签仍用内置公钥，指向假服务器也无法伪造凭证）。
pub const SERVER_URL: &str = "http://64.90.20.244:8443";

/// 与服务器私钥配对的公钥（编译期嵌入，发布产物不依赖外部文件）。
pub const PUBLIC_KEY_PEM: &str = include_str!("public_key.pem");

// 状态码与 license-guard Python 版 crypto.py 保持一致
pub const STATUS_OK: &str = "ok";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_CORRUPTED: &str = "corrupted";
pub const STATUS_SIGNATURE_INVALID: &str = "signature_invalid";
pub const STATUS_MACHINE_MISMATCH: &str = "machine_mismatch";
pub const STATUS_EXPIRED: &str = "expired";
pub const STATUS_CLOCK_ROLLBACK: &str = "clock_rollback";
pub const STATUS_FINGERPRINT_ERROR: &str = "fingerprint_error";

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 与 Python 版一致：去空白、统一大写。
fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<String>().to_uppercase()
}

/// SHA256(f"{tag}:{normalize(value)}")，64 位十六进制。
fn compute_machine_id(tag: &str, value: &str) -> String {
    let raw = format!("{tag}:{}", normalize(value));
    hex(&Sha256::digest(raw.as_bytes()))
}

// ---- 机器指纹（优先级：MachineGuid -> 主板 UUID -> CPU ProcessorId） ----

fn read_machine_guid() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        .ok()?;
    let val: String = key.get_value("MachineGuid").ok()?;
    let v = val.trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// 执行 `wmic <args>` 并取数据行（表头之后的第一个非空行）。
fn wmic_field(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("wmic")
        .args(args)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .nth(1)?;
    let header = args.last().copied().unwrap_or_default().to_lowercase();
    if line.to_lowercase() == header || line.is_empty() {
        return None;
    }
    Some(line.to_string())
}

/// 按优先级采集机器指纹；全部失败返回 Err（对应 STATUS_FINGERPRINT_ERROR）。
pub fn collect_machine_id() -> Result<String, ()> {
    if let Some(guid) = read_machine_guid() {
        return Ok(compute_machine_id("winreg_machineguid", &guid));
    }
    if let Some(uuid) = wmic_field(&["csproduct", "get", "UUID"]) {
        return Ok(compute_machine_id("wmic_csproduct_uuid", &uuid));
    }
    if let Some(pid) = wmic_field(&["cpu", "get", "ProcessorId"]) {
        return Ok(compute_machine_id("wmic_cpu_processorid", &pid));
    }
    Err(())
}

// ---- 凭证持久化 ----

fn license_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join(".license_guard").join("license.dat"))
}

/// 原子写入凭证：临时文件 + rename（Windows 上等价 os.replace）。
pub fn save_license(payload_b64: &str, signature_b64: &str) -> std::io::Result<()> {
    let path = license_path().ok_or_else(|| {
        std::io::Error::other("无法定位用户目录（USERPROFILE）")
    })?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let record = serde_json::json!({
        "payload_b64": payload_b64,
        "signature_b64": signature_b64,
        "last_seen": now_secs(),
    });
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, record.to_string())?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 校验通过后更新 last_seen（防时钟回拨），不改动已签名字段。尽力而为，失败静默。
fn touch_last_seen(path: &PathBuf) {
    let read = (|| -> std::io::Result<()> {
        let text = fs::read_to_string(path)?;
        let mut data: Value = serde_json::from_str(&text)?;
        data["last_seen"] = Value::from(now_secs());
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, data.to_string())?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    let _ = read;
}

// ---- RSA-SHA256(PKCS#1 v1.5) 验签 ----

fn verify_signature(canonical: &[u8], signature: &[u8]) -> bool {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::pkcs8::DecodePublicKey;
    use rsa::sha2::Sha256;
    use rsa::signature::Verifier;

    let Ok(key) = rsa::RsaPublicKey::from_public_key_pem(PUBLIC_KEY_PEM) else {
        return false;
    };
    let verifying_key = VerifyingKey::<Sha256>::new(key);
    let Ok(sig) = Signature::try_from(signature) else {
        return false;
    };
    verifying_key.verify(canonical, &sig).is_ok()
}

// ---- 本地凭证校验 ----

/// 校验本地凭证，返回状态码；指纹采集失败返回 Err（调用方映射为 fingerprint_error）。
pub fn check_license() -> Result<&'static str, ()> {
    let Some(path) = license_path() else {
        return Err(());
    };
    if !path.exists() {
        return Ok(STATUS_MISSING);
    }

    // 解析凭证文件
    let data = (|| -> Option<Value> {
        let text = fs::read_to_string(&path).ok()?;
        serde_json::from_str::<Value>(&text).ok()
    })();
    let Some(data) = data else {
        return Ok(STATUS_CORRUPTED);
    };
    let (Some(payload_b64), Some(signature_b64), Some(last_seen)) = (
        data.get("payload_b64").and_then(Value::as_str),
        data.get("signature_b64").and_then(Value::as_str),
        data.get("last_seen").and_then(Value::as_i64),
    ) else {
        return Ok(STATUS_CORRUPTED);
    };

    let engine = base64::engine::general_purpose::STANDARD;
    let (Ok(canonical), Ok(signature)) = (
        engine.decode(payload_b64),
        engine.decode(signature_b64),
    ) else {
        return Ok(STATUS_CORRUPTED);
    };

    // 公钥验签（在指纹采集前执行：坏凭证不必触发指纹错误）
    if !verify_signature(&canonical, &signature) {
        return Ok(STATUS_SIGNATURE_INVALID);
    }

    let Ok(payload) = serde_json::from_slice::<Value>(&canonical) else {
        return Ok(STATUS_CORRUPTED);
    };

    let machine = collect_machine_id()?;
    if payload.get("machine_id").and_then(Value::as_str) != Some(machine.as_str()) {
        return Ok(STATUS_MACHINE_MISMATCH);
    }

    let now = now_secs();
    let expires_at = payload.get("expires_at").and_then(Value::as_i64).unwrap_or(0);
    if now >= expires_at {
        return Ok(STATUS_EXPIRED);
    }
    if now < last_seen {
        return Ok(STATUS_CLOCK_ROLLBACK);
    }

    touch_last_seen(&path);
    Ok(STATUS_OK)
}

// ---- 远程激活 ----

/// 16 位十六进制 nonce（服务端仅作请求溯源，不校验唯一性）。
fn nonce_hex() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = format!("{t}-{}-{}", std::process::id(), now_secs());
    hex(&Sha256::digest(seed.as_bytes()))[..16].to_string()
}

fn server_url() -> String {
    std::env::var("LICENSE_GUARD_SERVER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| SERVER_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// 调用激活接口；成功返回 (payload_b64, signature_b64)，失败返回用户可读的错误信息。
pub fn activate(code: &str) -> Result<(String, String), String> {
    let machine = collect_machine_id().map_err(|_| "无法采集机器指纹".to_string())?;
    let url = format!("{}/api/activate", server_url());
    let body = serde_json::json!({
        "code": code,
        "machine_id": machine,
        "nonce": nonce_hex(),
    });

    let resp = ureq::post(&url)
        .timeout(Duration::from_secs(10))
        .send_json(body);

    match resp {
        Ok(r) => {
            let v: Value = r
                .into_json()
                .map_err(|e| format!("解析激活响应失败: {e}"))?;
            let payload = v
                .get("payload_b64")
                .and_then(Value::as_str)
                .ok_or_else(|| "激活响应缺少 payload_b64".to_string())?;
            let signature = v
                .get("signature_b64")
                .and_then(Value::as_str)
                .ok_or_else(|| "激活响应缺少 signature_b64".to_string())?;
            Ok((payload.to_string(), signature.to_string()))
        }
        Err(ureq::Error::Status(status, r)) => {
            // 403 口令错误 / 429 限流 / 422 参数校验失败
            let detail = r
                .into_json::<Value>()
                .ok()
                .and_then(|v| v.get("detail").cloned())
                .map(|d| match d {
                    Value::String(s) => s,
                    other => other.to_string(),
                })
                .unwrap_or_else(|| format!("激活失败（HTTP {status}）"));
            Err(detail)
        }
        Err(e) => Err(format!("无法连接验证服务器: {e}")),
    }
}
