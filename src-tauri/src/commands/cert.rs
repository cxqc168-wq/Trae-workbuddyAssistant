use std::os::windows::process::CommandExt;
use std::process::Command;
use tauri::{AppHandle, State};

use crate::python::spawn_script;
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

#[tauri::command]
pub fn cert_install(app: AppHandle, state: State<AppState>) -> Result<CertStatus, String> {
    // 1. 确保 CA 证书已生成（data_dir/certs/ca.cer）
    let cer = state.path("certs").join("ca.cer");
    if !cer.exists() {
        let _ = spawn_script(&state, "device_proxy.py", &["--gen-ca".to_string()], false)?
            .wait();
    }
    if !cer.exists() {
        return Err("CA 证书生成失败，无法安装".into());
    }
    let cer_arg = cer.to_string_lossy().replace('\\', "/").to_string();

    // 2. 以管理员权限安装到本地计算机受信任根证书颁发机构（触发 UAC）
    let ps = format!(
        "Start-Process certutil -ArgumentList '-addstore','-f','Root','{}' -Verb RunAs -Wait",
        cer_arg
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .creation_flags(0x08000000)
        .status()
        .map_err(|e| format!("启动证书安装失败: {e}"))?;

    if !status.success() {
        return Err("证书安装被取消或失败（可能需要管理员权限）".into());
    }
    let _ = app;
    Ok(cert_status(app, state))
}
