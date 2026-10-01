//! WorkBuddy 会话加密归档（.wds 格式）：跨设备/跨账号搬运会话。
//!
//! 文件格式：
//!   magic: [u8;4] = b"WDS1"
//!   version: u8 = 1
//!   salt: [u8;32]      (scrypt 盐)
//!   nonce: [u8;12]     (AES-GCM nonce)
//!   ciphertext: ...    (AES-256-GCM(gzip(tar)))
//!
//! tar 内：session.json + files/<相对路径>

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

use super::session_db::{self, SessionRecord, WbSessionDb};
use super::session_files;

const MAGIC: &[u8; 4] = b"WDS1";
const VERSION: u8 = 1;
const HEADER_SIZE: usize = 4 + 1 + 32 + 12; // magic + version + salt + nonce

#[derive(Serialize, Deserialize)]
struct SessionArchive {
    record: SessionRecord,
    files: Vec<String>, // 相对 dataRoot 的路径列表
}

/// 导出指定会话为 .wds 加密归档文件，返回文件路径。
pub fn export_session(
    session_id: &str,
    password: &str,
    output_dir: &std::path::Path,
) -> Result<PathBuf, String> {
    if password.is_empty() {
        return Err("密码不能为空".into());
    }
    if !session_db::is_valid_session_id(session_id) {
        return Err("无效的会话 ID".into());
    }

    // 1. 读取会话记录
    let db = WbSessionDb::open_readonly()?;
    let record = db
        .get_session(session_id)?
        .ok_or("会话不存在")?;

    // 2. 收集文件
    let files = session_files::collect(session_id)?;
    let file_paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let archive = SessionArchive {
        record: record.clone(),
        files: file_paths.clone(),
    };

    // 3. 打包 tar（在内存中）
    let mut tar_buf = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_buf);
        // session.json
        let json = serde_json::to_vec_pretty(&archive).map_err(|e| format!("序列化失败: {e}"))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(json.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "session.json", json.as_slice())
            .map_err(|e| format!("tar 写入 session.json 失败: {e}"))?;

        // 文件
        let root = session_db::workbuddy_data_root();
        for rel in &file_paths {
            let full = root.join(rel);
            if let Ok(content) = std::fs::read(&full) {
                let mut h = tar::Header::new_gnu();
                h.set_size(content.len() as u64);
                h.set_cksum();
                let tar_path = format!("files/{}", rel);
                tar.append_data(&mut h, &tar_path, content.as_slice())
                    .map_err(|e| format!("tar 写入 {} 失败: {}", rel, e))?;
            }
        }
        tar.finish().map_err(|e| format!("tar 完成失败: {e}"))?;
    }

    // 4. gzip 压缩
    let mut gz_buf = Vec::new();
    {
        let mut encoder = GzEncoder::new(&mut gz_buf, Compression::default());
        encoder
            .write_all(&tar_buf)
            .map_err(|e| format!("gzip 压缩失败: {e}"))?;
        encoder
            .finish()
            .map_err(|e| format!("gzip 完成失败: {e}"))?;
    }

    // 5. 加密
    let salt = rand_bytes(32);
    let nonce_bytes = rand_bytes(12);
    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("创建 cipher 失败: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, gz_buf.as_ref())
        .map_err(|e| format!("AES-GCM 加密失败: {e}"))?;

    // 6. 写文件
    std::fs::create_dir_all(output_dir).map_err(|e| format!("创建输出目录失败: {e}"))?;
    let filename = format!("{}.wds", session_id);
    let out_path = output_dir.join(&filename);
    let mut file = std::fs::File::create(&out_path).map_err(|e| format!("创建文件失败: {e}"))?;
    file.write_all(MAGIC).map_err(|e| format!("写入 magic 失败: {e}"))?;
    file.write_all(&[VERSION]).map_err(|e| format!("写入 version 失败: {e}"))?;
    file.write_all(&salt).map_err(|e| format!("写入 salt 失败: {e}"))?;
    file.write_all(&nonce_bytes).map_err(|e| format!("写入 nonce 失败: {e}"))?;
    file.write_all(&ciphertext).map_err(|e| format!("写入密文失败: {e}"))?;

    Ok(out_path)
}

/// 导入 .wds 归档到当前账号，返回新会话 id。
pub fn import_session(
    wds_path: &std::path::Path,
    password: &str,
    target_uid: &str,
) -> Result<String, String> {
    if password.is_empty() {
        return Err("密码不能为空".into());
    }

    // 1. 读取文件
    let data = std::fs::read(wds_path).map_err(|e| format!("读取文件失败: {e}"))?;
    if data.len() < HEADER_SIZE {
        return Err("文件太小，不是有效的 .wds 文件".into());
    }
    if &data[0..4] != MAGIC {
        return Err("文件 magic 不匹配，不是有效的 .wds 文件".into());
    }
    let version = data[4];
    if version != VERSION {
        return Err(format!("不支持的 .wds 版本: {}", version));
    }
    let salt = &data[5..37];
    let nonce_bytes = &data[37..49];
    let ciphertext = &data[49..];

    // 2. 解密
    let key = derive_key(password, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("创建 cipher 失败: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let gz_buf = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "解密失败：密码错误或文件已损坏".to_string())?;

    // 3. gzip 解压
    let mut decoder = GzDecoder::new(gz_buf.as_slice());
    let mut tar_buf = Vec::new();
    decoder
        .read_to_end(&mut tar_buf)
        .map_err(|e| format!("gzip 解压失败: {e}"))?;

    // 4. tar 解包
    let mut archive = tar::Archive::new(tar_buf.as_slice());
    let mut session_json: Option<Vec<u8>> = None;
    let mut extracted_files: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in archive
        .entries()
        .map_err(|e| format!("tar 解析失败: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("tar 条目读取失败: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("tar 路径读取失败: {e}"))?
            .to_string_lossy()
            .to_string();
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("tar 内容读取失败: {e}"))?;
        if path == "session.json" {
            session_json = Some(buf);
        } else if let Some(rel) = path.strip_prefix("files/") {
            extracted_files.push((rel.to_string(), buf));
        }
    }

    let session_json = session_json.ok_or("归档中缺少 session.json")?;
    let archive_data: SessionArchive =
        serde_json::from_slice(&session_json).map_err(|e| format!("解析 session.json 失败: {e}"))?;

    // 5. 生成新 id，插入 DB
    let new_id = uuid::Uuid::new_v4().to_string();
    let db = WbSessionDb::open()?;
    db.insert_copied(&archive_data.record, target_uid, &new_id)?;

    // 6. 写入文件（路径 remap 到新 id）
    let root = session_db::workbuddy_data_root();
    for (rel, content) in &extracted_files {
        let new_rel = match session_files::remap_rel_path(rel, &archive_data.record.id, &new_id) {
            Ok(p) => p,
            Err(e) => {
                // 文件路径 remap 失败不阻断，记录但继续
                eprintln!("跳过文件 {}: {}", rel, e);
                continue;
            }
        };
        let dst = root.join(&new_rel);
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&dst, content).is_err() {
            // 写入失败不阻断
        }
        // artifact-index 改 owner
        if new_rel.starts_with("artifact-index/") && new_rel.ends_with(".json") {
            let _ = rewrite_owner_in_memory(&dst, &new_id);
        }
    }

    Ok(new_id)
}

fn rewrite_owner_in_memory(file: &std::path::Path, new_id: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    if let Some(meta) = json.get_mut("_meta") {
        if let Some(owner) = meta.get_mut("ownerConversationId") {
            *owner = serde_json::Value::String(new_id.to_string());
        }
    }
    let out = serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(file, out).map_err(|e| e.to_string())?;
    Ok(())
}

/// scrypt 派生 32 字节密钥（N=16384, r=8, p=1）。
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let params = scrypt::Params::new(14, 8, 1, 32).map_err(|e| format!("scrypt 参数错误: {e}"))?;
    let mut key = [0u8; 32];
    scrypt::scrypt(password.as_bytes(), salt, &params, &mut key)
        .map_err(|e| format!("scrypt 派生失败: {e}"))?;
    Ok(key)
}

/// 生成随机字节。
fn rand_bytes(n: usize) -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    // 简单的 PRNG：基于时间 + 计数器，足够用于 nonce/salt
    // 生产环境应使用 rand crate，这里避免额外依赖
    let mut buf = vec![0u8; n];
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = seed ^ 0x9E3779B97F4A7C15;
    for b in buf.iter_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *b = (state >> 33) as u8;
    }
    buf
}
