//! Python 子进程管理辅助。
use std::path::{Path, PathBuf};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use crate::state::AppState;

/// Embedded Python must ignore host Python settings and use UTF-8 for IPC.
/// System Python in development retains its existing site-package behavior.
pub fn interpreter_args(executable: &str, python_dir: &Path) -> &'static [&'static str] {
    if Path::new(executable) == python_dir.join("python.exe") {
        &["-I", "-X", "utf8"]
    } else {
        &[]
    }
}

pub fn command(state: &AppState) -> Command {
    let mut command = Command::new(&state.python_exe);
    command.args(interpreter_args(&state.python_exe, &state.python_dir));
    command
}

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
    let mut cmd = command(state);
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
