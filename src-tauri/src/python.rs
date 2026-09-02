//! Python 子进程管理辅助。
use std::path::PathBuf;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use crate::state::AppState;

/// 启动一个 python 脚本，注入 TRAEDATA_DIR（指向应用数据目录）。
/// `script` 为 python 目录下的文件名（如 "device_proxy.py"）。
pub fn spawn_script(
    state: &AppState,
    script: &str,
    args: &[String],
    capture: bool,
) -> Result<std::process::Child, String> {
    let script_path: PathBuf = state.python_dir.join(script);
    if !script_path.exists() {
        return Err(format!("找不到脚本: {}", script_path.display()));
    }
    let data_dir = state.data_dir.to_string_lossy().to_string();
    let mut cmd = Command::new(&state.python_exe);
    cmd.arg(&script_path)
        .args(args)
        .creation_flags(0x08000000)
        .env("TRAEDATA_DIR", &data_dir)
        .env("PYTHONIOENCODING", "utf-8");
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    cmd.spawn().map_err(|e| format!("启动 {} 失败: {}", script, e))
}
