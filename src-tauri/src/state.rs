use std::path::PathBuf;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Mutex;

use crate::fs_utils;
use crate::models::Settings;

/// 应用全局状态。base_dir 指向 %APPDATA%\TraeWorkAssistant；
/// 子目录: conf/ (配置), data/ (数据), logs/ (日志)
/// python_dir 指向打包后的 python 脚本目录（Tauri resource `python/`）。
pub struct AppState {
    pub data_dir: PathBuf,
    pub python_dir: PathBuf,
    pub python_exe: String,
    /// JWT 刷新锁：防止多个并发请求同时 ExchangeToken
    pub jwt_refresh_lock: Mutex<()>,
}

/// 配置文件名列表（路由到 conf/ 目录）
const CONF_FILES: &[&str] = &["app_settings.json"];

impl AppState {
    pub fn new() -> Result<Self, String> {
        // 数据目录：%APPDATA%\TraeWorkAssistant，不存在则创建
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .map_err(|_| "无法读取 APPDATA 环境变量".to_string())?;
        let data_dir = appdata.join("TraeWorkAssistant");
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("创建数据目录失败: {e}"))?;

        // 创建子目录结构
        let conf_dir = data_dir.join("conf");
        let data_subdir = data_dir.join("data");
        let logs_dir = data_dir.join("logs");
        let _ = std::fs::create_dir_all(&conf_dir);
        let _ = std::fs::create_dir_all(&data_subdir);
        let _ = std::fs::create_dir_all(&logs_dir);

        // python 脚本目录：优先取 Tauri 资源目录下的 python/，否则回退到源码目录
        let python_dir = resolve_python_dir();

        // python 解释器：资源目录内嵌的 python.exe 优先；否则探测系统可用解释器。
        // Windows 官方安装通常提供 python.exe / py.exe，python3 反而常不存在，故依次探测。
        let embedded = python_dir.join("python.exe");
        let python_exe = if embedded.exists() {
            embedded.to_string_lossy().to_string()
        } else {
            probe_python_exe()
        };

        Ok(Self {
            data_dir,
            python_dir,
            python_exe,
            jwt_refresh_lock: Mutex::new(()),
        })
    }

    /// 配置文件路径：base_dir/conf/name
    pub fn conf_path(&self, name: &str) -> PathBuf {
        let dir = self.data_dir.join("conf");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    /// 数据文件路径：base_dir/data/name
    pub fn data_path(&self, name: &str) -> PathBuf {
        let dir = self.data_dir.join("data");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    /// 日志目录路径：base_dir/logs
    pub fn logs_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("logs");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 兼容性 path()：根据文件名自动路由到正确的子目录
    /// - 配置文件 → conf/
    /// - 数据文件 → data/
    /// - logs → logs/（日志根目录）
    /// - 其他 → data/（默认）
    pub fn path(&self, name: &str) -> PathBuf {
        if CONF_FILES.contains(&name) {
            self.conf_path(name)
        } else if name == "logs" {
            // 日志根目录：base_dir/logs
            self.logs_dir()
        } else if name == "profiles" || name == "certs" {
            // 数据子目录
            let dir = self.data_path(name);
            let _ = std::fs::create_dir_all(&dir);
            dir
        } else {
            self.data_path(name)
        }
    }

    pub fn settings(&self) -> Settings {
        // 统一走 fs_utils::read_json：文件缺失/为空/解析失败均回退默认，行为一致
        let mut s: Settings = fs_utils::read_json(&self.conf_path("app_settings.json"));
        // proxy_domains 为空时回填默认值，确保设置页始终展示默认监听域名
        if s.proxy_domains.trim().is_empty() {
            s.proxy_domains = crate::models::default_proxy_domains();
        }
        s
    }
}

/// 定位 python 脚本目录：覆盖安装版(MSI/NSIS)、便携版(zip 直接运行)、开发期三种布局。
fn resolve_python_dir() -> PathBuf {
    // 1) 运行期 Tauri 注入的资源目录：<RESOURCE_DIR>/python
    if let Ok(res) = std::env::var("TAURI_RESOURCE_DIR") {
        let p = PathBuf::from(res).join("python");
        if p.exists() {
            return p;
        }
    }
    // 2) 可执行文件周边布局（便携版 sidecar / 安装版均可能命中）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // dir/resources/python
            let c1 = dir.join("resources").join("python");
            if c1.exists() {
                return c1;
            }
            // dir/python（少数打包方式把 python 放 exe 同级）
            let c2 = dir.join("python");
            if c2.exists() {
                return c2;
            }
            // 上层再找 resources/python（如 exe 在 "<App>/Trae Work 助手.exe" 嵌套一层）
            if let Some(parent) = dir.parent() {
                let c3 = parent.join("resources").join("python");
                if c3.exists() {
                    return c3;
                }
            }
        }
    }
    // 3) 开发期：仓库 src-python
    PathBuf::from("src-python")
}

/// 定位 ps 脚本目录：与 python 目录同级，覆盖安装版/便携版/开发期。
pub fn resolve_ps_dir() -> PathBuf {
    let py = resolve_python_dir();
    // 安装/便携版：python 与 ps 都在 <RESOURCE_DIR> 下，故用 python 的父目录
    if let Some(parent) = py.parent() {
        let ps = parent.join("ps");
        if ps.exists() {
            return ps;
        }
    }
    // 开发期：python 在 src-python，ps 在 src-ps
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let c = dir.join("resources").join("ps");
            if c.exists() {
                return c;
            }
        }
    }
    PathBuf::from("src-ps")
}

/// 探测系统可用的 Python 解释器，依次尝试 python / python3 / py。
/// 均不可用时兜底返回 "python3"（保持原行为，由上层在启动时报错提示）。
fn probe_python_exe() -> String {
    for cand in ["python", "python3", "py"] {
        if Command::new(cand)
            .arg("--version")
            .creation_flags(0x08000000)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return cand.to_string();
        }
    }
    "python3".to_string()
}
