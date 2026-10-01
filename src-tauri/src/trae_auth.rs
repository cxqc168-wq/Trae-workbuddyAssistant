//! Trae 登录态管理：storage.json AES-128-CBC 加解密 + JWT 注入切换
//!
//! 参考 Trae-Work-CN-Account-Manager 项目的逆向算法：
//! - iCubeAuthInfo 使用 AES-128-CBC + HMAC-SHA512 加密
//! - 密钥派生：SHA512(SHA512(embedded_key) || (jQ XOR wQ))[0..32] -> key[0..16], iv[16..32]
//! - 存储路径：%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use aes::Aes128;
use base64::Engine;
use cbc::{Decryptor, Encryptor};
use rand::RngCore;
use sha2::{Digest, Sha512};

use crate::fs_utils;

// ==================== 加密常量（来自 Trae 源码逆向） ====================

const JQ: [u8; 64] = [
    82, 9, 106, 213, 48, 54, 165, 56, 191, 64, 163, 158, 129, 243, 215, 251, 124, 227, 57, 130,
    155, 47, 255, 135, 52, 142, 67, 68, 196, 222, 233, 203, 84, 123, 148, 50, 166, 194, 35, 61,
    238, 76, 149, 11, 66, 250, 195, 78, 8, 46, 161, 102, 40, 217, 36, 178, 118, 91, 162, 73, 109,
    139, 209, 37,
];

const WQ: [u8; 64] = [
    31, 221, 168, 51, 136, 7, 199, 49, 177, 18, 16, 89, 39, 128, 236, 95, 96, 81, 127, 169, 25,
    181, 74, 13, 45, 229, 122, 159, 147, 201, 156, 239, 160, 224, 59, 77, 174, 42, 245, 176, 200,
    235, 187, 60, 131, 83, 153, 97, 23, 43, 4, 126, 186, 119, 214, 38, 225, 105, 20, 99, 85, 33,
    12, 125,
];

const HEADER_MAGIC_T: u8 = b't';
const HEADER_MAGIC_C: u8 = b'c';
const HEADER_VERSION: u8 = 5;
const HEADER_SIZE: usize = 6; // magic(2) + version(1) + reserved(3)
const EMBEDDED_KEY_SIZE: usize = 32;
const HMAC_SIZE: usize = 64; // SHA-512

/// Trae 数据目录
pub fn trae_data_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let base = PathBuf::from(appdata);
    let candidates = ["TRAE SOLO CN", "TRAE SOLO", "Trae", "Trae CN"];
    for name in &candidates {
        let dir = base.join(name);
        if dir
            .join("User")
            .join("globalStorage")
            .join("storage.json")
            .exists()
            || dir.join("machineid").exists()
        {
            return dir;
        }
    }
    base.join("TRAE SOLO CN")
}

/// storage.json 路径
pub fn storage_json_path() -> PathBuf {
    trae_data_dir()
        .join("User")
        .join("globalStorage")
        .join("storage.json")
}

/// machineid 文件路径
pub fn machineid_path() -> PathBuf {
    trae_data_dir().join("machineid")
}

// ==================== 加解密 ====================

/// 派生 AES-128 密钥和 IV
fn derive_key_iv(embedded_key: &[u8]) -> ([u8; 16], [u8; 16]) {
    // AES_CONSTANT = jQ XOR wQ
    let mut aes_constant = [0u8; 64];
    for i in 0..64 {
        aes_constant[i] = JQ[i] ^ WQ[i];
    }

    // o = SHA-512(embedded_key)
    let mut hasher = Sha512::new();
    hasher.update(embedded_key);
    let o = hasher.finalize();

    // n = o || aes_constant (128 bytes)
    let mut n = [0u8; 128];
    n[0..64].copy_from_slice(&o);
    n[64..128].copy_from_slice(&aes_constant);

    // c = SHA-512(n)
    let mut hasher = Sha512::new();
    hasher.update(&n);
    let c = hasher.finalize();

    let key: [u8; 16] = c[0..16].try_into().unwrap();
    let iv: [u8; 16] = c[16..32].try_into().unwrap();
    (key, iv)
}

/// 解密 iCubeAuthInfo（Base64 -> 明文 JSON）
pub fn decrypt_auth_info(encrypted_b64: &str) -> Result<String, String> {
    let encrypted_data = base64::engine::general_purpose::STANDARD
        .decode(encrypted_b64)
        .map_err(|e| format!("Base64 解码失败: {e}"))?;

    let total_header = HEADER_SIZE + EMBEDDED_KEY_SIZE; // 38
    if encrypted_data.len() < total_header + 16 {
        return Err(format!("加密数据太短: {} bytes", encrypted_data.len()));
    }

    // 验证头部
    if encrypted_data[0] != HEADER_MAGIC_T || encrypted_data[1] != HEADER_MAGIC_C {
        return Err("加密数据头部魔数不正确".to_string());
    }
    if encrypted_data[2] != HEADER_VERSION {
        return Err(format!("不支持的加密版本: {}", encrypted_data[2]));
    }

    // 提取嵌入的密钥
    let embedded_key = &encrypted_data[HEADER_SIZE..total_header];
    let (key, iv) = derive_key_iv(embedded_key);

    // AES-128-CBC 解密
    let ciphertext = &encrypted_data[total_header..];
    if ciphertext.len() % 16 != 0 {
        return Err(format!(
            "加密数据长度异常（{} 字节，非 16 的倍数）",
            ciphertext.len()
        ));
    }

    let mut decryptor = Decryptor::<Aes128>::new(&key.into(), &iv.into());
    let mut padded = vec![0u8; ciphertext.len()];
    for (chunk, out_chunk) in ciphertext.chunks(16).zip(padded.chunks_mut(16)) {
        let mut block = aes::Block::default();
        block.copy_from_slice(chunk);
        let mut out_block = aes::Block::default();
        decryptor.decrypt_block_b2b_mut(&block, &mut out_block);
        out_chunk.copy_from_slice(&out_block);
    }

    // 移除 PKCS7 填充
    let pad_len = *padded.last().ok_or("解密数据为空")? as usize;
    if pad_len == 0 || pad_len > 16 {
        return Err(format!("无效的 PKCS7 填充: {pad_len}"));
    }
    for i in 0..pad_len {
        if padded[padded.len() - 1 - i] != pad_len as u8 {
            return Err("PKCS7 填充验证失败".to_string());
        }
    }
    let decrypted = &padded[..padded.len() - pad_len];

    if decrypted.len() < HMAC_SIZE {
        return Err(format!("解密数据太短: {} bytes", decrypted.len()));
    }

    // 验证 HMAC-SHA512
    let hmac_expected = &decrypted[..HMAC_SIZE];
    let plaintext = &decrypted[HMAC_SIZE..];
    let mut hasher = Sha512::new();
    hasher.update(plaintext);
    let hmac_computed = hasher.finalize();
    if hmac_expected != hmac_computed.as_slice() {
        return Err("HMAC 验证失败".to_string());
    }

    String::from_utf8(plaintext.to_vec()).map_err(|e| format!("UTF-8 解码失败: {e}"))
}

/// 加密 iCubeAuthInfo（明文 JSON -> Base64）
pub fn encrypt_auth_info(plaintext: &str) -> Result<String, String> {
    // 生成随机 32 字节 embedded_key
    let mut embedded_key = [0u8; EMBEDDED_KEY_SIZE];
    rand::thread_rng().fill_bytes(&mut embedded_key);
    let (key, iv) = derive_key_iv(&embedded_key);

    // HMAC-SHA512(plaintext)
    let plaintext_bytes = plaintext.as_bytes();
    let mut hasher = Sha512::new();
    hasher.update(plaintext_bytes);
    let hmac = hasher.finalize();

    // 拼接 HMAC || plaintext
    let mut data = Vec::with_capacity(HMAC_SIZE + plaintext_bytes.len());
    data.extend_from_slice(&hmac);
    data.extend_from_slice(plaintext_bytes);

    // PKCS7 填充
    let pad_len = 16 - (data.len() % 16);
    let pad_byte = pad_len as u8;
    for _ in 0..pad_len {
        data.push(pad_byte);
    }

    // AES-128-CBC 加密
    let mut encryptor = Encryptor::<Aes128>::new(&key.into(), &iv.into());
    let mut ciphertext = vec![0u8; data.len()];
    for (chunk, out_chunk) in data.chunks(16).zip(ciphertext.chunks_mut(16)) {
        let mut block = aes::Block::default();
        block.copy_from_slice(chunk);
        let mut out_block = aes::Block::default();
        encryptor.encrypt_block_b2b_mut(&block, &mut out_block);
        out_chunk.copy_from_slice(&out_block);
    }

    // 构造完整数据: header(6) + embedded_key(32) + ciphertext
    let mut output = Vec::with_capacity(HEADER_SIZE + EMBEDDED_KEY_SIZE + ciphertext.len());
    output.push(HEADER_MAGIC_T);
    output.push(HEADER_MAGIC_C);
    output.push(HEADER_VERSION);
    output.push(0x10); // reserved
    output.push(0x00);
    output.push(0x00);
    output.extend_from_slice(&embedded_key);
    output.extend_from_slice(&ciphertext);

    Ok(base64::engine::general_purpose::STANDARD.encode(&output))
}

// ==================== 登录信息结构 ====================

/// Trae 登录信息（用于构建 iCubeAuthInfo）
#[derive(Debug, Clone)]
pub struct TraeLoginInfo {
    pub token: String,
    pub refresh_token: Option<String>,
    pub user_id: String,
    pub email: String,
    pub username: String,
    pub avatar_url: String,
}

/// 构建 iCubeAuthInfo JSON
fn build_auth_info_json(info: &TraeLoginInfo) -> Result<serde_json::Value, String> {
    // 账号库使用 HTTP Authorization 格式；客户端会自行添加 Cloud-IDE-JWT 前缀。
    let trimmed = info.token.trim();
    let token = trimmed
        .strip_prefix("Cloud-IDE-JWT ")
        .or_else(|| trimmed.strip_prefix("Bearer "))
        .unwrap_or(trimmed)
        .trim();
    let jwt = crate::jwt::parse(token);
    if token.split('.').count() != 3 || jwt.user_id.as_deref() != Some(info.user_id.as_str()) {
        return Err("JWT 格式无效或与目标账号不匹配，请重新提取该账号的登录凭据".into());
    }
    let now = chrono::Utc::now();
    let expired_at = jwt
        .exp_timestamp
        .and_then(|exp| chrono::DateTime::from_timestamp(exp, 0))
        .ok_or("JWT 缺少有效的过期时间，请重新提取该账号的登录凭据")?;
    if expired_at <= now {
        return Err("JWT 已过期，请先刷新 JWT 或重新登录提取，当前客户端登录态未修改".into());
    }
    let refresh_expired_at = now + chrono::Duration::days(180);

    Ok(serde_json::json!({
        "token": token,
        "refreshToken": info.refresh_token.clone().unwrap_or_default(),
        "expiredAt": expired_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        "refreshExpiredAt": refresh_expired_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        "tokenReleaseAt": now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        "userId": info.user_id,
        "host": "https://api.trae.cn",
        "userRegion": {
            "region": "CN",
            "_aiRegion": "CN"
        },
        "account": {
            "username": info.username,
            "iss": "",
            "iat": 0,
            "organization": "",
            "work_country": "",
            "email": info.email,
            "avatar_url": info.avatar_url,
            "description": "",
            "scope": "marscode",
            "loginScope": "trae",
            "storeCountryCode": "cn",
            "storeCountrySrc": "uid",
            "storeRegion": "CN",
            "userTag": "row"
        }
    }))
}

/// 构建 iCubeEntitlementInfo JSON（默认免费权益）
fn build_entitlement_info_json() -> serde_json::Value {
    serde_json::json!({
        "identityStr": "Free",
        "identity": 0,
        "isPayFreshman": false,
        "isSupportCommercialization": true,
        "hasPackage": false,
        "enableEntitlement": true,
        "detail": {
            "can_gen_solo_code": false,
            "fast_request_per": 1,
            "in_wait": false,
            "permission": 1,
            "toast_read": false,
            "toastRead": false,
            "canGenSoloCode": false,
            "fastRequestPer": 1,
            "inWaitlist": false
        }
    })
}

// ==================== 进程管理 ====================

/// 检查指定名称的进程是否在运行
fn is_process_running(name: &str) -> bool {
    let filter = format!("IMAGENAME eq {name}");
    let out = Command::new("tasklist")
        .args(["/FI", &filter, "/NH"])
        .creation_flags(0x08000000)
        .output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains(name)
        }
        Err(_) => false,
    }
}

/// 关闭 Trae 进程（排除本应用自身）
pub fn kill_trae(custom_exe: Option<&str>) -> Result<(), String> {
    let mut names = vec![
        "TRAE SOLO CN.exe".to_string(),
        "TRAE SOLO.exe".to_string(),
        "Trae.exe".to_string(),
        "Trae Work CN.exe".to_string(),
    ];
    if let Some(exe) = custom_exe {
        if let Some(file_name) = std::path::Path::new(exe)
            .file_name()
            .and_then(|f| f.to_str())
        {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(file_name)) {
                names.insert(0, file_name.to_string());
            }
        }
    }

    for name in &names {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", name, "/T"])
            .creation_flags(0x08000000)
            .output();
    }

    // 等待进程完全退出（最多等 3 秒）
    for _ in 0..10 {
        let any_running = names.iter().any(|name| is_process_running(name));
        if !any_running {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    Err("Trae 进程未能完全退出，请关闭客户端后重试（可能需要使用相同权限运行助手）".into())
}

/// 切换前解析启动路径，防止关闭客户端、改写登录态后才发现无法重启。
fn resolve_trae_exe(custom_exe: Option<&str>) -> Result<String, String> {
    if let Some(exe) = custom_exe.map(str::trim).filter(|exe| !exe.is_empty()) {
        if !Path::new(exe).is_file() {
            return Err(format!(
                "配置的 Trae 路径不存在: {exe}，请在设置中重新选择可执行文件"
            ));
        }
        return Ok(exe.to_string());
    }
    let (installed, path, _) = crate::commands::env::detect_trae(custom_exe.map(|s| s.to_string()));
    if !installed || path.is_none() {
        return Err(
            "未找到 Trae 可执行文件，请在「设置 → 代理与签到」中配置 Trae 路径".to_string(),
        );
    }

    Ok(path.unwrap())
}

/// 启动已经预检查的客户端，并检测启动后立即退出的失败。
fn start_trae(exe: &str) -> Result<(), String> {
    let mut child = Command::new(exe)
        .current_dir(Path::new(exe).parent().ok_or("Trae 路径缺少安装目录")?)
        .env_remove("ELECTRON_RUN_AS_NODE")
        .spawn()
        .map_err(|e| format!("启动 Trae 失败 ({}): {e}", exe))?;
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("检查 Trae 启动状态失败: {e}"))?
        {
            return Err(format!("Trae 启动后立即退出 ({status})，请查看客户端日志"));
        }
    }
    Ok(())
}

// ==================== 浏览器数据清除 ====================

/// 清除可能包含旧登录会话的浏览器数据
/// 保留 state.vscdb（IDE 设置），只清除登录相关数据
fn clear_browser_data(data_dir: &Path) {
    let files_to_remove = ["Local State", "Network/Cookies", "Network/Cookies-journal"];
    let dirs_to_remove = ["IndexedDB", "Local Storage", "Session Storage"];

    for f in &files_to_remove {
        let p = data_dir.join(f);
        if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
    }
    for d in &dirs_to_remove {
        let p = data_dir.join(d);
        if p.exists() {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}

// ==================== storage.json 操作 ====================

/// 读取 storage.json
fn read_storage_json() -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let path = storage_json_path();
    if !path.exists() {
        return Ok(serde_json::Map::new());
    }
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("读取 storage.json 失败: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("解析 storage.json 失败: {e}"))?;
    json.as_object()
        .cloned()
        .ok_or_else(|| "storage.json 格式错误".to_string())
}

/// 写入 storage.json
fn write_storage_json(obj: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let path = storage_json_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let content =
        serde_json::to_string_pretty(obj).map_err(|e| format!("序列化 JSON 失败: {e}"))?;
    std::fs::write(&path, content).map_err(|e| format!("写入 storage.json 失败: {e}"))?;
    Ok(())
}

/// 生成新的机器码和遥测 ID
fn generate_new_device_ids() -> (String, String, String, String) {
    let machine_id = uuid::Uuid::new_v4().to_string();
    let hash = md5_hash(machine_id.as_bytes());
    let telemetry_machine_id = hex::encode(hash);
    let sqm_id = format!("{{{}}}", uuid::Uuid::new_v4().to_string().to_uppercase());
    let dev_device_id = uuid::Uuid::new_v4().to_string();
    (machine_id, telemetry_machine_id, sqm_id, dev_device_id)
}

/// 简单 MD5 哈希（用于 telemetry.machineId）
fn md5_hash(data: &[u8]) -> [u8; 16] {
    let mut hasher = md5::Md5::new();
    hasher.update(data);
    hasher.finalize().into()
}

// ==================== 核心切换逻辑 ====================

/// 切换 Trae 账号（JWT 注入方式）
///
/// 流程：
/// 1. 关闭 Trae
/// 2. 生成新的机器码
/// 3. 清除浏览器登录数据（Cookies/IndexedDB/LocalStorage 等）
/// 4. 更新 storage.json：移除旧登录信息，写入新的加密 iCubeAuthInfo
/// 5. 启动 Trae
pub fn switch_account(
    info: &TraeLoginInfo,
    data_dir: &Path,
    custom_exe: Option<&str>,
) -> Result<(), String> {
    fs_utils::app_log(
        data_dir,
        &format!("trae_auth: 开始切换账号 user_id={}", info.user_id),
    );

    // 所有可预检查的失败都在关闭客户端及修改文件之前返回。
    let auth_info = build_auth_info_json(info)?;
    let auth_plain =
        serde_json::to_string(&auth_info).map_err(|e| format!("序列化 auth_info 失败: {e}"))?;
    let auth_encrypted = encrypt_auth_info(&auth_plain)?;
    let exe = resolve_trae_exe(custom_exe)?;
    read_storage_json()?;
    fs_utils::app_log(data_dir, &format!("trae_auth: 预检查通过，启动路径={exe}"));

    // 1. 关闭 Trae
    kill_trae(Some(&exe))?;
    fs_utils::app_log(data_dir, "trae_auth: Trae 进程已关闭");

    // 进程退出时可能落盘新状态，读取最终内容后再修改文件。
    let mut storage = read_storage_json()?;

    let trae_dir = trae_data_dir();

    // 2. 生成并写入新的机器码
    let (machine_id, telemetry_machine_id, sqm_id, dev_device_id) = generate_new_device_ids();
    let machineid_file = machineid_path();
    if let Some(parent) = machineid_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&machineid_file, &machine_id)
        .map_err(|e| format!("写入 machineid 失败: {e}"))?;
    fs_utils::app_log(data_dir, &format!("trae_auth: 新机器码={machine_id}"));

    // 3. 清除浏览器登录数据
    clear_browser_data(&trae_dir);
    fs_utils::app_log(data_dir, "trae_auth: 浏览器登录数据已清除");

    // 4. 更新 storage.json
    // 移除所有旧的登录和权益信息
    storage.retain(|k, _| {
        !k.starts_with("iCubeAuthInfo://")
            && !k.starts_with("iCubeEntitlementInfo://")
            && !k.starts_with("iCubeServerData://")
    });

    // 更新遥测 ID
    storage.insert(
        "telemetry.machineId".to_string(),
        serde_json::Value::String(telemetry_machine_id),
    );
    storage.insert(
        "telemetry.sqmId".to_string(),
        serde_json::Value::String(sqm_id),
    );
    storage.insert(
        "telemetry.devDeviceId".to_string(),
        serde_json::Value::String(dev_device_id),
    );

    // 构建并加密新的登录信息
    storage.insert(
        "iCubeAuthInfo://icube.cloudide".to_string(),
        serde_json::Value::String(auth_encrypted),
    );

    let entitlement = build_entitlement_info_json();
    storage.insert(
        "iCubeEntitlementInfo://icube.cloudide".to_string(),
        serde_json::Value::String(serde_json::to_string(&entitlement).unwrap_or_default()),
    );

    write_storage_json(&storage)?;
    fs_utils::app_log(
        data_dir,
        "trae_auth: storage.json 已更新（新登录信息已写入）",
    );

    // 5. 启动 Trae
    start_trae(&exe)?;
    fs_utils::app_log(data_dir, "trae_auth: Trae 已启动");

    Ok(())
}

/// 从当前 Trae 读取登录信息（用于保存当前登录态）
pub fn read_current_login() -> Result<TraeLoginInfo, String> {
    let storage = read_storage_json()?;
    let auth_str = storage
        .get("iCubeAuthInfo://icube.cloudide")
        .and_then(|v| v.as_str())
        .or_else(|| {
            storage
                .iter()
                .find(|(k, _)| k.starts_with("iCubeAuthInfo://"))
                .and_then(|(_, v)| v.as_str())
        })
        .ok_or_else(|| "未找到 iCubeAuthInfo".to_string())?;

    // 尝试解密（加密数据），如果失败则尝试直接解析 JSON（明文）
    let auth_json: serde_json::Value = match decrypt_auth_info(auth_str) {
        Ok(decrypted) => {
            serde_json::from_str(&decrypted).map_err(|e| format!("解密后 JSON 解析失败: {e}"))?
        }
        Err(_) => serde_json::from_str(auth_str)
            .map_err(|e| format!("auth_info 既不是有效加密也不是明文 JSON: {e}"))?,
    };

    let token = auth_json
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "未找到 token".to_string())?
        .to_string();

    let user_id = auth_json
        .get("userId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "未找到 userId".to_string())?
        .to_string();

    let refresh_token = auth_json
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let email = auth_json
        .get("account")
        .and_then(|a| a.get("email"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let username = auth_json
        .get("account")
        .and_then(|a| a.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let avatar_url = auth_json
        .get("account")
        .and_then(|a| a.get("avatar_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(TraeLoginInfo {
        token,
        refresh_token,
        user_id,
        email,
        username,
        avatar_url,
    })
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    fn login_fixture(token_prefix: &str) -> TraeLoginInfo {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"data":{"id":"12345"},"exp":4102444800}"#);
        TraeLoginInfo {
            token: format!("{token_prefix}e30.{payload}.signature"),
            refresh_token: None,
            user_id: "12345".into(),
            email: "test@example.com".into(),
            username: "test".into(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn injected_token_does_not_duplicate_client_authorization_prefix() {
        for prefix in ["Cloud-IDE-JWT ", "Bearer ", ""] {
            let info = login_fixture(prefix);
            let auth = build_auth_info_json(&info).unwrap();
            let encrypted = encrypt_auth_info(&auth.to_string()).unwrap();
            let stored: serde_json::Value =
                serde_json::from_str(&decrypt_auth_info(&encrypted).unwrap()).unwrap();
            let expected_token = login_fixture("").token;
            assert_eq!(stored["token"], expected_token);
            assert_eq!(
                format!("Cloud-IDE-JWT {}", stored["token"].as_str().unwrap()),
                format!("Cloud-IDE-JWT {expected_token}")
            );
        }
    }

    #[test]
    fn injected_expiry_matches_jwt_instead_of_fabricated_fourteen_days() {
        let auth = build_auth_info_json(&login_fixture("Cloud-IDE-JWT ")).unwrap();
        assert_eq!(auth["expiredAt"], "2100-01-01T00:00:00.000Z");
    }

    #[test]
    fn expired_jwt_is_rejected_before_injection() {
        let mut info = login_fixture("");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"data":{"id":"12345"},"exp":1}"#);
        info.token = format!("e30.{payload}.signature");
        assert!(build_auth_info_json(&info).unwrap_err().contains("已过期"));
    }

    #[test]
    fn jwt_for_another_account_is_rejected_before_injection() {
        let mut info = login_fixture("");
        info.user_id = "another-account".into();
        assert!(build_auth_info_json(&info).unwrap_err().contains("不匹配"));
    }

    #[test]
    fn jwt_without_expiry_is_rejected_before_injection() {
        let mut info = login_fixture("");
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"data":{"id":"12345"}}"#);
        info.token = format!("e30.{payload}.signature");
        assert!(build_auth_info_json(&info)
            .unwrap_err()
            .contains("过期时间"));
    }

    #[test]
    fn configured_executable_is_used_and_missing_path_does_not_fall_back() {
        let exe = std::env::current_exe().unwrap();
        let path = exe.to_str().unwrap();
        assert_eq!(
            resolve_trae_exe(Some(&format!("  {path}  "))).unwrap(),
            path
        );
        let missing = exe.parent().unwrap().join("missing-trae-switch-test.exe");
        assert!(resolve_trae_exe(Some(missing.to_str().unwrap())).is_err());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let plaintext = r#"{"token":"test123","userId":"12345"}"#;
        let encrypted = encrypt_auth_info(plaintext).unwrap();
        let decrypted = decrypt_auth_info(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_invalid_header() {
        let result = decrypt_auth_info("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_key_iv_deterministic() {
        let key = [0u8; 32];
        let (k1, i1) = derive_key_iv(&key);
        let (k2, i2) = derive_key_iv(&key);
        assert_eq!(k1, k2);
        assert_eq!(i1, i2);
    }

    #[test]
    fn test_detect_trae() {
        let (installed, path, _) = crate::commands::env::detect_trae(None);
        if installed {
            assert!(path.is_some());
        }
    }
}
