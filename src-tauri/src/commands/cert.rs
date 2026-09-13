use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, State};

use crate::state::AppState;

#[derive(serde::Serialize)]
pub struct CertStatus {
    pub installed: bool,
}

#[tauri::command]
pub fn cert_status(_app: AppHandle, _state: State<AppState>) -> CertStatus {
    let out = Command::new("certutil")
        .args(["-store", "Root"])
        .creation_flags(0x08000000)
        .output();
    let installed = match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains("TraeDeviceProxyCA")
        }
        Err(_) => false,
    };
    CertStatus { installed }
}

/// 在 Rust 内原生生成自签 CA（与 device_proxy.py `ensure_ca()` 的输出格式兼容）：
/// certs/ca.crt（PEM 证书）、certs/ca.key（PKCS#1 PEM 私钥）、certs/ca.cer（DER 证书）。
///
/// 背景：安装包只打包了 python 脚本而未内嵌解释器，此前依赖
/// `python device_proxy.py --gen-ca` 生成证书，在未安装 Python 的 Windows 11 上
/// `python3` 命令是 WindowsApps 存根（打印 "Python was not found" 后退出）或根本
/// 不存在，导致生成静默失败、无法安装证书，故改为 Rust 原生实现。
fn ensure_ca_files(state: &AppState) -> Result<PathBuf, String> {
    let cert_dir = state.path("certs");
    let crt = cert_dir.join("ca.crt");
    let key = cert_dir.join("ca.key");
    let cer = cert_dir.join("ca.cer");
    if crt.exists() && key.exists() && cer.exists() {
        return Ok(cer);
    }
    generate_ca_to(&cert_dir)?;
    Ok(cer)
}

/// 在指定目录内生成自签 CA 三件套（ca.crt / ca.key / ca.cer）。
fn generate_ca_to(cert_dir: &std::path::Path) -> Result<(), String> {
    let crt = cert_dir.join("ca.crt");
    let key = cert_dir.join("ca.key");
    let cer = cert_dir.join("ca.cer");

    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;
    use rsa::rand_core::OsRng;

    // RSA 2048 + SHA256 自签、CN=TraeDeviceProxyCA、CA 基本约束与密钥用法，
    // 与 device_proxy.py 生成的 CA 保持一致（Python 端 load_pem_private_key
    // 兼容 PKCS#1/PEM 私钥，代理启动时可直接复用本 CA 签发叶子证书）
    let rsa_key = rsa::RsaPrivateKey::new(&mut OsRng, 2048)
        .map_err(|e| format!("生成 CA 密钥失败: {e}"))?;
    let pkcs8_pem = rsa_key
        .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(|e| format!("序列化 CA 密钥失败: {e}"))?;
    let key_pair = rcgen::KeyPair::from_pkcs8_pem_and_sign_algo(
        pkcs8_pem.as_str(),
        &rcgen::PKCS_RSA_SHA256,
    )
    .map_err(|e| format!("初始化 CA 密钥对失败: {e}"))?;

    let mut params = rcgen::CertificateParams::new(Vec::new())
        .map_err(|e| format!("初始化证书参数失败: {e}"))?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "TraeDeviceProxyCA");
    params.distinguished_name = dn;
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::DigitalSignature,
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    // 与 Python 端一致：notBefore 回拨 1 天容忍时钟偏差，有效期 10 年
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(3650);

    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("生成 CA 证书失败: {e}"))?;

    std::fs::create_dir_all(cert_dir).map_err(|e| format!("创建证书目录失败: {e}"))?;
    std::fs::write(&crt, cert.pem()).map_err(|e| format!("写入 ca.crt 失败: {e}"))?;
    let key_pem = rsa_key
        .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(|e| format!("序列化 CA 私钥失败: {e}"))?;
    std::fs::write(&key, key_pem.as_bytes()).map_err(|e| format!("写入 ca.key 失败: {e}"))?;
    std::fs::write(&cer, cert.der()).map_err(|e| format!("写入 ca.cer 失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成 CA 三件套的冒烟测试：文件齐全、格式可被外部工具解析
    #[test]
    fn generate_ca_smoke() {
        let dir = std::env::temp_dir().join("twa_ca_compat_test");
        let _ = std::fs::remove_dir_all(&dir);
        generate_ca_to(&dir).expect("generate_ca_to failed");
        for f in ["ca.crt", "ca.key", "ca.cer"] {
            assert!(dir.join(f).exists(), "{f} 未生成");
        }
        // ca.crt 应为 PEM 证书、ca.key 应为 PKCS#1 PEM 私钥、ca.cer 应为 DER 证书
        let crt = std::fs::read_to_string(dir.join("ca.crt")).unwrap();
        assert!(crt.contains("BEGIN CERTIFICATE"), "ca.crt 不是 PEM 证书");
        let key = std::fs::read_to_string(dir.join("ca.key")).unwrap();
        assert!(key.contains("BEGIN RSA PRIVATE KEY"), "ca.key 不是 PKCS#1 私钥");
        let der = std::fs::read(dir.join("ca.cer")).unwrap();
        assert_eq!(&der[..1], &[0x30], "ca.cer 不是 DER（非 SEQUENCE 起始）");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[tauri::command]
pub fn cert_install(app: AppHandle, state: State<AppState>) -> Result<CertStatus, String> {
    // 1. 原生生成 CA 证书（data_dir/certs/ca.cer，不依赖 Python）
    let cer = ensure_ca_files(&state)?;
    let cer_arg = cer.to_string_lossy().to_string();

    // 2. 以管理员权限安装到本地计算机受信任根证书颁发机构（触发 UAC）
    let ps = format!(
        "Start-Process certutil -ArgumentList '-addstore','-f','Root','{cer_arg}' -Verb RunAs -Wait"
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .creation_flags(0x08000000)
        .status()
        .map_err(|e| format!("启动证书安装失败: {e}"))?;

    if !status.success() {
        return Err("证书安装被取消或失败（可能拒绝了 UAC 管理员授权，请重试并在弹窗中选择\"是\"）".into());
    }

    // 3. 复核安装结果：Start-Process -Wait 不透传 certutil 退出码，
    //    需回读系统根证书存储确认，避免"假成功"
    let st = cert_status(app, state);
    if !st.installed {
        return Err("证书已提交安装，但未在系统根证书存储中检测到，请重试".into());
    }
    Ok(st)
}
