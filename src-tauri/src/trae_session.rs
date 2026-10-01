//! Trae 对话解密导出模块。
//!
//! Trae 会话库 `%APPDATA%\TRAE SOLO CN\ModularData\ai-agent\database.db` 是 SQLCipher4 加密：
//!   AES-256-CBC / PBKDF2-HMAC-SHA512×256000 / page_size=4096 / reserve=80
//!
//! 密钥不从配置文件读取，而是从运行中的 Trae 进程内存扫描 `x'<64hex>'` 模式取得，
//! 再用首页 HMAC 校验确认。解密后读取 chat_session / chat_message 表导出对话。

use aes::Aes256;
use cbc::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use serde::Serialize;
use sha2::Sha512;
use std::path::PathBuf;

type Aes256CbcDec = cbc::Decryptor<Aes256>;
type HmacSha512 = Hmac<Sha512>;

const PAGE_SIZE: usize = 4096;
const RESERVE_SIZE: usize = 80;
const HMAC_SIZE: usize = 48;
const IV_SIZE: usize = 16;
const PBKDF2_ITERATIONS: u32 = 256_000;

/// Trae 数据库路径（国内版）。
pub fn trae_db_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(appdata)
        .join("TRAE SOLO CN")
        .join("ModularData")
        .join("ai-agent")
        .join("database.db")
}

// ==================== 进程内存扫描 ====================

/// 查找 Trae 进程 PID（按进程名模糊匹配）。
pub fn find_trae_pid() -> Option<u32> {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)],
                );
                let lower = name.to_lowercase();
                if lower.contains("trae") && !lower.contains("traeworkassistant") {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

/// 从进程内存扫描所有 `x'<64hex>'` 模式的候选密钥，返回 32 字节密钥列表。
pub fn scan_keys_from_process(pid: u32) -> Result<Vec<Vec<u8>>, String> {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::memoryapi::{ReadProcessMemory, VirtualQueryEx};
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::winnt::{MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE, PAGE_READONLY, PAGE_READWRITE, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

    unsafe {
        let handle = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid);
        if handle.is_null() {
            return Err(format!("无法打开进程 {}（请确保 Trae 正在运行且以相同权限运行）", pid));
        }

        let mut candidates: Vec<Vec<u8>> = Vec::new();
        let mut addr: usize = 0;
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();

        loop {
            let ret = VirtualQueryEx(
                handle,
                addr as *const _,
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            );
            if ret == 0 {
                break;
            }

            if mbi.State == MEM_COMMIT
                && (mbi.Protect == PAGE_READWRITE || mbi.Protect == PAGE_READONLY)
                && mbi.Type == MEM_PRIVATE
            {
                let region_size = mbi.RegionSize;
                if region_size > 0 && region_size < 256 * 1024 * 1024 {
                    let mut buf = vec![0u8; region_size];
                    let mut bytes_read = 0usize;
                    if ReadProcessMemory(
                        handle,
                        mbi.BaseAddress,
                        buf.as_mut_ptr() as *mut _,
                        region_size,
                        &mut bytes_read,
                    ) != 0
                        && bytes_read > 64
                    {
                        scan_buffer_for_keys(&buf[..bytes_read], &mut candidates);
                    }
                }
            }

            addr = mbi.BaseAddress as usize + mbi.RegionSize;
            if addr == 0 || mbi.RegionSize == 0 {
                break;
            }
        }

        CloseHandle(handle);
        candidates.dedup();
        Ok(candidates)
    }
}

/// 在内存缓冲区中扫描 `x'<64hex>'` 模式。
fn scan_buffer_for_keys(buf: &[u8], candidates: &mut Vec<Vec<u8>>) {
    let pattern = b"x'";
    let mut i = 0;
    while i + 67 < buf.len() {
        if buf[i] == pattern[0] && buf[i + 1] == pattern[1] {
            // 检查接下来 64 个字符是否都是 hex
            let hex_start = i + 2;
            let hex_end = hex_start + 64;
            if hex_end < buf.len() && buf[hex_end] == b'\'' {
                let hex_str = &buf[hex_start..hex_end];
                if hex_str.iter().all(|b| b.is_ascii_hexdigit()) {
                    if let Ok(key) = hex::decode(hex_str) {
                        if key.len() == 32 && !candidates.contains(&key) {
                            candidates.push(key);
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

// ==================== SQLCipher4 页面解密 ====================

#[derive(Clone)]
pub struct DerivedKeys {
    pub encryption_key: [u8; 32],
    pub hmac_key: [u8; 32],
}

/// 从 raw key 派生 SQLCipher4 密钥。
/// raw key 是 32 字节，encryption_key = raw_key，hmac_key = PBKDF2(raw_key, salt=raw_key[0:16])。
pub fn derive_keys(raw_key: &[u8]) -> Result<DerivedKeys, String> {
    if raw_key.len() != 32 {
        return Err("raw key 必须是 32 字节".into());
    }
    let mut encryption_key = [0u8; 32];
    encryption_key.copy_from_slice(raw_key);

    let mut hmac_key = [0u8; 32];
    pbkdf2_hmac::<Sha512>(
        raw_key,
        &raw_key[0..16],
        PBKDF2_ITERATIONS,
        &mut hmac_key,
    );

    Ok(DerivedKeys {
        encryption_key,
        hmac_key,
    })
}

/// 验证密钥是否正确（解密第一页并校验 HMAC）。
pub fn verify_key(db_path: &std::path::Path, keys: &DerivedKeys) -> bool {
    let data = match std::fs::read(db_path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.len() < PAGE_SIZE {
        return false;
    }
    decrypt_page(&data[0..PAGE_SIZE], 1, keys).is_ok()
}

/// 解密单个页面（page_number 从 1 开始）。
fn decrypt_page(page: &[u8], page_number: u32, keys: &DerivedKeys) -> Result<Vec<u8>, String> {
    if page.len() != PAGE_SIZE {
        return Err(format!("页面大小不符: {} != {}", page.len(), PAGE_SIZE));
    }

    let reserve_start = PAGE_SIZE - RESERVE_SIZE;
    let reserve = &page[reserve_start..];

    // reserve 布局: [0..48] HMAC, [48..64] IV, [64..68] page_size(BE), [68..80] padding
    let hmac_stored = &reserve[0..HMAC_SIZE];
    let iv = &reserve[HMAC_SIZE..HMAC_SIZE + IV_SIZE];
    let encrypted_data = &page[0..reserve_start];

    // 1. HMAC 校验：HMAC-SHA512(hmac_key, encrypted_data + page_number_be)
    let mut mac = HmacSha512::new_from_slice(&keys.hmac_key)
        .map_err(|e| format!("创建 HMAC 失败: {e}"))?;
    mac.update(encrypted_data);
    mac.update(&page_number.to_be_bytes());
    let computed = mac.finalize().into_bytes();
    if &computed[0..HMAC_SIZE] != hmac_stored {
        return Err("HMAC 校验失败".into());
    }

    // 2. AES-256-CBC 解密
    let decrypted = Aes256CbcDec::new(&keys.encryption_key.into(), iv.into())
        .decrypt_padded_vec_mut::<NoPadding>(encrypted_data)
        .map_err(|e| format!("AES 解密失败: {e}"))?;

    Ok(decrypted)
}

/// 解密整个数据库，输出明文 SQLite 到临时文件，返回路径。
pub fn decrypt_database(
    db_path: &std::path::Path,
    keys: &DerivedKeys,
    output_path: &std::path::Path,
) -> Result<PathBuf, String> {
    let data = std::fs::read(db_path).map_err(|e| format!("读取数据库失败: {e}"))?;
    if data.len() < PAGE_SIZE {
        return Err("数据库文件太小".into());
    }
    let num_pages = data.len() / PAGE_SIZE;
    let mut plaintext = Vec::with_capacity(data.len());

    for i in 0..num_pages {
        let page_start = i * PAGE_SIZE;
        let page = &data[page_start..page_start + PAGE_SIZE];
        let decrypted = decrypt_page(page, (i + 1) as u32, keys)?;
        plaintext.extend_from_slice(&decrypted);
        // 补回 reserve 区（明文数据库不需要 reserve，填充 0）
        plaintext.extend_from_slice(&[0u8; RESERVE_SIZE]);
    }

    // 修正第一页的 page_size 字段（SQLite header 偏移 16-17，大端）
    if plaintext.len() >= 18 {
        plaintext[16] = ((PAGE_SIZE - RESERVE_SIZE) >> 8) as u8;
        plaintext[17] = ((PAGE_SIZE - RESERVE_SIZE) & 0xFF) as u8;
    }

    std::fs::write(output_path, &plaintext).map_err(|e| format!("写入明文数据库失败: {e}"))?;
    Ok(output_path.to_path_buf())
}

// ==================== 对话导出 ====================

#[derive(Serialize)]
pub struct TraeChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct TraeChatSession {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

/// 从解密后的明文数据库导出所有会话和消息。
pub fn export_chats(plain_db_path: &std::path::Path) -> Result<Vec<TraeChatSession>, String> {
    let conn = rusqlite::Connection::open_with_flags(
        plain_db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开明文数据库失败: {e}"))?;

    // 读取会话列表
    let mut sessions = Vec::new();
    let sql = "SELECT id, title, created_at, updated_at FROM chat_session ORDER BY created_at DESC;";
    let mut stmt = conn.prepare(sql).map_err(|e| format!("准备查询失败: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TraeChatSession {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: 0,
            })
        })
        .map_err(|e| format!("查询会话失败: {e}"))?;
    for r in rows {
        if let Ok(s) = r {
            sessions.push(s);
        }
    }

    // 读取消息
    for session in &mut sessions {
        let msg_sql = "SELECT id, role, content, created_at FROM chat_message WHERE chat_session_id = ? ORDER BY created_at ASC;";
        let count = match conn.prepare(msg_sql) {
            Ok(mut msg_stmt) => msg_stmt
                .query_map([&session.id], |row| {
                    Ok(TraeChatMessage {
                        id: row.get(0)?,
                        session_id: session.id.clone(),
                        role: row.get(1)?,
                        content: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })
                .map(|msgs| msgs.filter_map(|m| m.ok()).count())
                .unwrap_or(0),
            Err(_) => 0,
        };
        session.message_count = count;
    }

    Ok(sessions)
}

/// 导出指定会话的完整消息为 JSON 字符串。
pub fn export_session_messages(
    plain_db_path: &std::path::Path,
    session_id: &str,
) -> Result<Vec<TraeChatMessage>, String> {
    let conn = rusqlite::Connection::open_with_flags(
        plain_db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开明文数据库失败: {e}"))?;

    let sql = "SELECT id, role, content, created_at FROM chat_message WHERE chat_session_id = ? ORDER BY created_at ASC;";
    let mut stmt = conn.prepare(sql).map_err(|e| format!("准备查询失败: {e}"))?;
    let messages: Vec<TraeChatMessage> = stmt
        .query_map([session_id], |row| {
            Ok(TraeChatMessage {
                id: row.get(0)?,
                session_id: session_id.to_string(),
                role: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| format!("查询消息失败: {e}"))?
        .filter_map(|m| m.ok())
        .collect();

    Ok(messages)
}

/// 完整流程：扫描密钥 → 验证 → 解密 → 导出会话列表。
/// 返回 (解密后的明文数据库路径, 会话列表)。
pub fn trae_session_list(output_dir: &std::path::Path) -> Result<(PathBuf, Vec<TraeChatSession>), String> {
    let db_path = trae_db_path();
    if !db_path.exists() {
        return Err(format!("未找到 Trae 数据库：{}", db_path.display()));
    }

    let pid = find_trae_pid().ok_or("未找到运行中的 Trae 进程，请先启动 Trae")?;
    let candidates = scan_keys_from_process(pid)?;
    if candidates.is_empty() {
        return Err("未在 Trae 进程内存中找到候选密钥".into());
    }

    // 逐个尝试密钥
    let mut working_keys: Option<DerivedKeys> = None;
    for raw in &candidates {
        if let Ok(keys) = derive_keys(raw) {
            if verify_key(&db_path, &keys) {
                working_keys = Some(keys);
                break;
            }
        }
    }
    let keys = working_keys.ok_or("所有候选密钥均无法解密数据库（HMAC 校验失败）")?;

    // 解密
    std::fs::create_dir_all(output_dir).map_err(|e| format!("创建输出目录失败: {e}"))?;
    let plain_path = output_dir.join("trae_plain.db");
    decrypt_database(&db_path, &keys, &plain_path)?;

    // 导出
    let sessions = export_chats(&plain_path)?;
    Ok((plain_path, sessions))
}
