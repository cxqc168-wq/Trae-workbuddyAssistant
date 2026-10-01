# 会话迁移 / 对话记录 / 账号切换增强 — 设计文档

- 日期：2026-09-14
- 状态：待评审
- 关联：账号切换（`src-tauri/src/commands/switch.rs`、`profile.rs`）、WorkBuddy 模块（`src-tauri/src/workbuddy/`）、账号页（`src/pages/Accounts.tsx`、`src/pages/WorkBuddyAccounts.tsx`）
- 参考开源项目（源码已逐行研读）：
  - `yiyiqd/trae-session-export` —— Trae 加密会话库的密钥提取、页面级解密、对话重建、Markdown 导出
  - `xhrxgr/Trae-Work-CN-Account-Manager` —— Trae storage.json 原生加密写入、多开实例隔离、单实例切换
  - `babygoton/WorkDaddy` —— WorkBuddy 登录文件切换、SQLite 会话复制、lineage 跨账号同步、`.wds` 加密归档导入导出

---

## 1. 背景与目标

### 1.1 现状能力盘点（本项目 v2.4.6）

| 能力 | Trae | WorkBuddy |
|---|---|---|
| 账号 token 管理（签到/积分/API） | ✅ `checkin_accounts.json` | ✅ `workbuddy_accounts.json` |
| 客户端登录态切换 | ✅ 依赖 PowerShell 整机快照（`trae-switch-bridge.ps1`，`data/profiles/<uid>/`） | ❌ `workbuddy/auth_file.rs` **只读**，仅导入 token，不写回官方登录文件 |
| 多开实例隔离 | ❌ | ❌ |
| 对话记录保存/导出 | ❌ | ❌ |
| 切号后会话迁移（对话跟随账号） | ❌ | ❌ |
| 会话加密归档 / 跨设备导入导出 | ❌ | ❌ |

**核心缺口**：WorkBuddy 侧连"客户端账号切换"都没有（只能切 token 用于签到，不能让 WorkBuddy 客户端本身换号）；两个平台都没有对话记录保存与迁移能力。

### 1.2 目标能力矩阵（本次新增）

| 编号 | 能力 | 平台 | 优先级 | 技术来源 |
|---|---|---|---|---|
| A | WorkBuddy 客户端账号切换（登录文件备份/原子写回 + 进程刷新） | WB | P0 | WorkDaddy `lib.js/switchTo` |
| B | WorkBuddy 会话迁移：切号后把对话复制到目标账号（SQLite + 文件） | WB | P0 | WorkDaddy `daemon.js/copySessionRecord` |
| C | WorkBuddy 会话加密归档导入/导出（`.wds` 等价物） | WB | P1 | WorkDaddy `session-transfer.js/secure-transfer.js` |
| D | Trae 对话记录解密、浏览、导出 Markdown/JSON | Trae | P1 | trae-session-export |
| E | Trae storage.json 原生加密写入切换（Rust 化，作为 PS 快照的可选替代/补充） | Trae | P2 | Account-Manager `machine.rs` |
| F | Trae 多开实例隔离（`--user-data-dir`） | Trae | P2 | Account-Manager `instance_manager.rs` |
| G | Trae 跨账号会话迁移（实验性，重新加密写回） | Trae | P3 / POC | trae-session-export + Account-Manager |

> 优先级原则：**先把 WorkBuddy 全链路补齐（A→B→C），再做 Trae 的"只读导出"（D，零风险确定性高），最后才是 Trae 写回/多开（E/F）与实验性迁移（G）。**

### 1.3 两个平台会话存储的本质差异（决定了为什么 WB 能"迁移"而 Trae 先做"导出"）

- **WorkBuddy**：会话库是**明文 SQLite**（`~/.workbuddy/workbuddy.db` 的 `sessions` 表）+ 明文 JSONL/文件，会话归属仅由 `sessions.user_id` 决定。复制一行记录、换一个新 UUID、把 `user_id` 改成目标账号、再把对应文件路径改名，客户端即可见 —— 迁移是确定性操作。
- **Trae**：会话库是 **SQLCipher 4 加密**（`ModularData/ai-agent/database.db`），密钥只存在于运行进程内存；且会话与服务端 `conversation_id`、工作区路径存在绑定。**只读解密导出**完全可行；但"解密→改归属→重新加密写回让另一账号可见"受服务端校验影响，不保证成功，故降级为实验特性（G）。

---

## 2. 关键调研结论（机制拆解，作为后续设计的事实依据）

### 2.1 Trae 会话库：加密参数与解密链路（trae-session-export）

**数据库路径（需与本项目 `switch.rs` 的 `TRAE_DIR_NAMES = [Trae, Trae CN, TraeCN, TRAE SOLO CN]` 探测对齐）：**

| 产品 | 路径 |
|---|---|
| Trae Work（SOLO 国内） | `%APPDATA%\TRAE SOLO CN\ModularData\ai-agent\database.db` |
| Trae CN | `%APPDATA%\Trae CN\ModularData\ai-agent\database.db` |

**SQLCipher 4 参数（写库 PRAGMA 与解密都用这套）：**

- 算法 `AES-256-CBC`；KDF `PBKDF2-HMAC-SHA512`，迭代 `256000`
- 页大小 `4096`；每页保留区 `reserve = 80`（IV 16 字节 + HMAC 64 字节）；HMAC 算法 `HMAC-SHA512`；小端页号

**密钥提取（必须在 IDE 运行时）：**

1. 打开进程 `TRAE SOLO CN.exe` / `Trae CN.exe`（Windows API：`CreateToolhelp32Snapshot` 枚举 + `OpenProcess(PROCESS_VM_READ|QUERY_INFORMATION)` + `ReadProcessMemory` 分块扫描）。
2. 在内存中匹配形如 `x'<64 个十六进制>'` 的字节串（即 32 字节原始密钥的 SQL hex-literal）。
3. 用第 1 页做 HMAC 校验筛选正确密钥：
   - `salt = db[0..16]`；`key = PBKDF2_HMAC_SHA512(raw_key, salt, 256000, 32)`；`hmac_key = SHA512(key)`
   - 第 1 页：密文数据 `data = page[0 .. 4096-80]`、`iv = page[4016 .. 4032]`、存储 HMAC = `page[4032..]` 取小端前 32 字节
   - `calc = HMAC_SHA512(hmac_key, little_endian_u32(page_no) || data || iv)`，比对前 32 字节。

**页面级解密（不依赖 sqlcipher native 库即可只读导出）：**

- 对每一页用上面的 `key` 做 `AES-256-CBC` 解密（IV 取该页 reserve 前 16 字节），拼接后即得到一个标准明文 SQLite 文件（第 1 页前 16 字节 salt 区域需清零/保留 SQLite header）。
- 该方案只用到 `pbkdf2 + sha2 + aes` 纯算法 crate，**不需要** `sqlcipher3`/native 编译，适合只读导出（能力 D）。
- 只有"写回实时加密库"（能力 G 删除/改归属）才需要真正的 SQLCipher 驱动（见 §7 依赖选型）。

**解密后核心表：**

| 表 | 作用 | 关键字段 |
|---|---|---|
| `chat_session` | 会话列表 | `session_id` PK、`session_title`、`created_at`、`updated_at` |
| `chat_message` | 消息元数据 | `message_id`、`session_id`、`message_role`(user/assistant)、`message_index`、`start_time`、`deleted_at` |
| `chat_message_general` | 用户输入 | `message_id`、`content`(JSON：`text_content.text` 或 `text`) |
| `chat_message_task` | 助手任务 | `message_id`、`content`(JSON：`messages[].plan_item.tool_call_info{name,params}`、`summary`) |
| `history_v2` | 助手过程 | `message_id`、`messages`(JSON：`raw_messages[]{role,content,tool_calls}`) |
| `server_history_info` | 服务端增量流 | `conversation_id`、`messages`(JSON：`raw_messages[]{role,content,tool_calls[].function_call{name,arguments}}`) |

**对话重建优先级（trae-session-export 已验证）：**

1. 优先 `server_history_info`：按时间戳 merge 增量流、展开 `raw_messages`，信息最全（含完整工具调用参数）。
2. 回退 `history_v2`（助手过程）+ `chat_message_general`（用户输入）+ `chat_message_task.summary`（助手摘要）拼接。
3. 工具调用渲染：识别 `Write/Edit/RunCommand/Read/Grep/Glob` 等，转成 Markdown 代码块/折叠块。

### 2.2 Trae 账号切换的两种范式（Account-Manager）

**范式一：多开实例隔离（能力 F）**

- 命令行：`TRAE SOLO CN.exe --user-data-dir <每实例独立目录> --extensions-dir <共享插件目录>`。
- 每个实例在自己的 `user-data-dir` 内拥有独立 `machineid` + `storage.json`，互不影响，可同时在线；`extensions-dir` 共享避免重复装插件。不动系统注册表。

**范式二：单实例切换 + storage.json 原生加密写入（能力 E）**

`storage.json` 路径 `<data>/User/globalStorage/storage.json`，其中键 `iCubeAuthInfo://icube.cloudide` 的值是加密 token。Account-Manager 用 Rust 复现了官方加解密（可直接移植，与本项目技术栈一致）：

```
AES_CONSTANT = JQ XOR WQ            // 两个 64 字节硬编码常量逐字节异或（源码 machine.rs）
embedded_key = random(32)           // 每次加密随机
o = SHA512(embedded_key)
c = SHA512(o || AES_CONSTANT)
aes_key = c[0..16] ; iv = c[16..32] // AES-128-CBC
plaintext = SHA512(raw_json)[64] || raw_json        // 前缀 64 字节自校验（无密钥普通 SHA512）
cipher = AES_128_CBC_ENC(PKCS7(plaintext), aes_key, iv)
out = base64( header[6] || embedded_key[32] || cipher )
header = [116, 99, 5, 0, 0, 0]      // 't','c',version=5 + 3 reserved
```

切换写入步骤：① 写 `machineid`（新 uuid4）；② `storage.json` 移除旧 `iCubeAuthInfo://icube.cloudide`、`iCubeAuthInfo://usertag`；③ 写入新 `iCubeAuthInfo`（上面加密）与明文 `iCubeEntitlementInfo`；④ `telemetry.machineId = md5(machineid)`；⑤ **保留 `state.vscdb`**（IDE 设置/工作区状态不丢）；⑥ 清缓存目录后重启。

> 与本项目现有 PowerShell 快照范式的关系：PS 快照是"整目录备份/还原 + 6 层设备标识重置"，覆盖面广但重、依赖脚本与提权；Rust 原生写入是"只改登录相关键"，轻、快、可多开。二者**并存**，E 作为高级选项，不替换现有稳定链路。

### 2.3 WorkBuddy 登录文件与会话体系（WorkDaddy，能力 A/B/C 的直接蓝本）

**（1）登录文件（能力 A）**

- Windows 路径：`%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info`（国际版为 `workbuddy-desktop-ai.info`）。
- 内容为 JSON：`{ account: { uid, nickname, uin, phoneNumber, ... }, auth: { accessToken, refreshToken, domain, expiresAt, ... } }`，`account.uid` 是账号唯一标识。
- 切换原理：把当前登录文件原样按 `accounts/<uid>.info` 备份；切换时把目标备份经 `tmp → rename` **原子替换**回官方固定文件（`0o600` 权限），然后通过 CDP `Page.reload` 刷新 WorkBuddy 渲染进程即可生效，**无需重启整个客户端**。

**（2）会话存储（能力 B）**

- 数据根 `dataRoot = ~/.workbuddy`；会话库 `sessionDb = ~/.workbuddy/workbuddy.db`（明文 SQLite，表 `sessions`）。
- `sessions` 表复制时涉及的 21 列（`SESSION_COPY_COLUMNS`，源码已固定）：
  `id, cwd, user_id, title, custom_title, status, created_at, updated_at, last_activity_at, is_playground, source_mode, is_background_automation, mode, model, expert_id, expert_locale, expert_runtime_identity, expert_marketplace, permission_mode, use_sandbox_cli, project_id`
- 与会话关联的 **5 类磁盘文件**（相对 `dataRoot`，迁移时按 session id 改名）：

| 类别 | 相对路径 | 处理 |
|---|---|---|
| 消息正文 | `projects/<projectHash>/<id>.jsonl` | 复制为 `<newId>.jsonl` |
| 会话附属目录 | `projects/<projectHash>/<id>/` | 递归复制为 `<newId>/` |
| 工作区会话 | `workspace/sessions/<id>/` | 递归复制 |
| 任务数据 | `tasks/<id>/` | 递归复制 |
| 文件历史 | `file-history/<id>/` | 递归复制 |
| 产物索引 | `artifact-index/<id>.json` | 复制为 `<newId>.json`，**并把内部 `_meta.ownerConversationId` 改写为 newId**（否则官方列表会把产物过滤掉） |

**（3）单会话迁移算法（`copySessionRecord` / `insertCopiedSession`）**

```
newId = uuid4()
INSERT INTO sessions(21 列) VALUES (newId, src.cwd, 目标uid, src.title, ..., 时间戳重置 updated_at=now)
复制上述 5 类文件：oldId 路径 → newId 路径（artifact-index 同步改 ownerConversationId）
```

**（4）lineage 跨账号同步（防止重复 + 双向跟随，能力 B 的关键）**

- 同一对话在 A/B/C 账号下的多个副本归属同一个 `lineageId`，成员关系与 `uid→targetSessionId` 映射持久化。
- 切号时对每个 lineage：选**文件内容 mtime 最新**的成员作为源，同步到其它成员（已存在则更新文件而非新建），从而"A 账号聊的内容切到 B 也能看到，再切回 A 不丢、不产生重复会话"。
- 幂等保护：切换前先查 `mapping` 与 `lineage 成员`，命中已有目标会话则只更新文件；用 `(lineageId, targetUid)` 维度的串行锁 + 全局任务队列，快速连续切号也不会重复插入。

**（5）自动复制队列（切号后异步执行，不卡 UI）**

- `/api/switch` 只负责切登录文件并立即返回；随后 `startAutoCopyJob(sourceUid, targetUid)` 入队，**串行**执行：重新规划计划 → 逐会话 `copySessionRecord` → lineage 对账。
- 复制规则三选一：全部会话 / 指定会话 / 按工作区（cwd）匹配。任务暴露 `total/processed/copied/skipped/partial/failed` 进度。
- 每个会话之间主动让出事件循环，避免大量 SQLite/文件 I/O 饿死界面。

**（6）`.wds` 加密归档（能力 C，手动跨设备/备份导入导出）**

- 文件格式（v4）：`MAGIC(8, "WDS4\r\n\x1a\n") + salt(16) + iv(12) + gcmTag(16)` 共 52 字节头；其后是 AES-256-GCM 密文。
- 密钥：`scrypt(password, salt)` 派生 32 字节；GCM 的 AAD = 头部前 36 字节（magic+salt+iv），防头部篡改。
- 明文为 gzip 流，内部是**长度前缀帧**：每帧 `u32BE 长度 + JSON`；帧类型 `archive`（归档头）/`session`（会话 record + 文件数）/`file`（相对路径 + 大小 + **原始字节，不 Base64**，支持超大附件流式处理）。
- 限制：单次 ≤100 会话、单会话 ≤20000 文件、元数据帧 ≤1 MiB。
- 导入侧安全：**先完整验证 GCM tag 再解压**；逐帧校验路径（拒绝 `..` 穿越、绝对路径、符号链接、重复路径）；文件以独占方式 `wx` 创建、`0o600`；同样走"新 UUID + 改 `user_id` + 路径 remap + artifact owner 改写"。兼容旧 v2/v3 JSON 加密包。

---

## 3. 总体设计

### 3.1 模块地图（新增 Rust 模块，全部挂在既有 Tauri 架构下）

```
src-tauri/src/
├── workbuddy/                       # 既有模块（签到/积分/HTTP），本次扩展
│   ├── auth_file.rs                 # 【改】从"只读"扩展为"读 + 备份 + 原子写回切换"（能力 A）
│   ├── session_db.rs               # 【新】workbuddy.db 访问层：连接、参数化查询、列映射（能力 B）
│   ├── session_files.rs            # 【新】5 类会话文件收集/复制/remap/artifact owner 改写（能力 B/C）
│   ├── session_lineage.rs          # 【新】lineage 成员/映射持久化、对账、选最新成员（能力 B）
│   ├── session_transfer.rs         # 【新】.wds 归档：scrypt+AES-GCM+gzip+长度帧（能力 C）
│   └── session_jobs.rs             # 【新】切号后自动复制串行队列 + 进度事件（能力 B）
├── trae_session/                    # 【新目录】Trae 对话能力（能力 D/G）
│   ├── mod.rs
│   ├── paths.rs                    # 多产品目录探测（对齐 switch.rs TRAE_DIR_NAMES）
│   ├── mem_scan.rs                 # Windows 进程内存扫描提取 SQLCipher 密钥
│   ├── decrypt.rs                  # PBKDF2/SHA512/AES-CBC 页面级解密 → 明文 SQLite 字节
│   ├── schema.rs                   # 6 张表的结构体与 SQL
│   ├── rebuild.rs                  # 对话重建（server_history_info 优先，history_v2 回退）
│   ├── export_md.rs                # Markdown / JSON 导出 + 工具调用渲染
│   └── crypto_write.rs             # 【P3】iCubeAuthInfo 加解密 + 加密库写回（能力 E/G）
├── commands/
│   ├── workbuddy_session.rs        # 【新】WB 会话/切换/导入导出 Tauri 命令
│   └── trae_session.rs             # 【新】Trae 对话浏览/导出 Tauri 命令
└── main.rs                         # 注册新命令
```

### 3.2 数据流总览

**WorkBuddy 切号 + 会话跟随（A+B）：**

```
前端点"切换到账号B"
 → wb_switch_account(uid, {reload:true, migrate:'auto'|'ask'|'off'})
 → 备份当前官方 auth 文件到 data/wb_auth/<currentUid>.info（若未备份）
 → tmp+rename 原子写回 B 的 auth 文件
 → （可选）CDP reload WorkBuddy 窗口；不可用则提示重启
 → 若 migrate=auto：session_jobs 入队 sourceUid→targetUid
     → 读 workbuddy.db 源账号 sessions（按规则筛选）
     → 逐会话：新 uuid + INSERT(user_id=B) + 复制 5 类文件 + artifact owner 改写
     → lineage 对账，emit wb-session-job 进度
 → 前端进度条展示 copied/skipped/failed
```

**Trae 对话导出（D，只读零风险）：**

```
前端"Trae 对话存档"页 → wb/trae_session_list(product)
 → 检测 IDE 是否运行 → 内存扫描取密钥（失败则让用户选择已导出的明文 db 或手动填密钥）
 → 页面级解密到临时明文 db（内存/临时目录，用完即删）
 → 读 chat_session 列表返回前端
 → 用户勾选 → trae_session_export({ids, format:'md'|'json', outDir})
 → 对话重建 + 渲染 → 写文件 → 返回导出清单
```

### 3.3 与现有代码的边界约定

- **不改动**签到/积分/API 网关/代理等既有稳定逻辑；新模块只读复用 `workbuddy::accounts`、`fs_utils`、`jwt`。
- WorkBuddy 现有 `workbuddy_accounts.json`（用于签到的 token 集合）与新增的"客户端登录态切换"是两件事：前者是**本应用持有的 token 库**，后者是**操作官方客户端登录文件**。账号以 `uid` 关联，UI 上合并展示但存储分离。
- Trae 现有 PowerShell 切换链路保持不动；能力 E（Rust 原生写入）作为设置页"切换引擎"可选项，默认仍走 PS。

---

## 4. 模块详细设计 — 能力 A：WorkBuddy 客户端账号切换

### 4.1 扩展 `workbuddy/auth_file.rs`

新增（保留现有只读函数）：

```rust
// 官方登录文件（区分国内/国际；复用 profiles 思路，先只做国内 workbuddy-desktop.info）
pub fn auth_file_path() -> PathBuf;                              // 已存在
pub fn parse_auth_info(bytes) -> Option<WbAuthInfo>;            // 结构化解析 uid/token/...

// 本应用托管目录：%APPDATA%\TraeWorkAssistant\data\wb_auth\<uid>.info
fn auth_store_dir() -> PathBuf;
pub fn backup_current_auth() -> Result<String, String>;         // 读官方文件→按 uid 落盘，返回 uid
pub fn stored_auth_path(uid: &str) -> PathBuf;
pub fn list_stored_auth() -> Vec<StoredAuthMeta>;               // 列出已备份登录文件（脱敏）
pub fn write_auth_atomic(target: &Path, content: &[u8]) -> Result<(),String>; // tmp+rename, 0o600
pub fn switch_auth_to(uid: &str) -> Result<WbAuthInfo, String>; // 校验 uid 一致→原子写回
```

**安全要点（源自 WorkDaddy）：**

- 备份文件名严格限定 `<uid>.info`：basename 校验、禁止 `.tmp` 后缀、禁止路径分隔符与空字节。
- 写回前校验备份 JSON 的 `account.uid == 请求 uid`，不一致直接中止（防止错号）。
- 一律 `写 tmp → fsync → rename` 原子替换，避免写一半损坏官方登录文件；文件权限 `0600`。
- token 不出现在日志；前端只拿脱敏 meta（与现有 `account_meta` 一致）。

### 4.2 进程刷新（替代 WorkDaddy 的 CDP reload）

WorkDaddy 靠注入后的 CDP 连接 reload 渲染进程；本项目**不做注入**，按成本从低到高三档：

1. **优先：优雅重启 WorkBuddy**（Rust 枚举并结束 `WorkBuddy.exe` 进程树 → 重新 `spawn` 安装路径 exe），逻辑与 `switch.rs` 里 `tr_is_trae_running`/重启 Trae 同构，最稳。
2. 可选增强：若未来通过启动参数拿到 CDP 端口，则 `Page.reload`，实现不重启切换（列为后续优化，不阻塞 P0）。
3. 都失败：返回提示"登录文件已切换，请手动重启 WorkBuddy"。

### 4.3 Tauri 命令（`commands/workbuddy_session.rs`）

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `wb_auth_list` | — | `StoredAuthMeta[]` | 已备份的官方登录文件列表 |
| `wb_auth_backup_current` | — | `{uid}` | 把当前官方登录文件入库 |
| `wb_switch_account` | `uid, opts{reload, migrate}` | `{uid, nickname, reloaded, jobId?}` | 写回登录文件 + 可选刷新 + 触发迁移队列 |

命令用 `#[tauri::command(async)]`，文件 I/O 放阻塞线程，避免卡 UI（与 `workbuddy_checkin_all` 一致）。

---

## 5. 模块详细设计 — 能力 B：WorkBuddy 会话迁移

### 5.1 `session_db.rs`：SQLite 访问层

**依赖选型**：用 `rusqlite`（bundled，静态编译，免用户装 SQLite），不采用 WorkDaddy 的"node:sqlite / sqlite3 CLI 回退"方案（那是 Node 环境约束，Rust 下 `rusqlite bundled` 最干净）。

```rust
pub struct WbSessionDb { conn: rusqlite::Connection }   // 指向 ~/.workbuddy/workbuddy.db

impl WbSessionDb {
    pub fn open(data_root: &Path) -> Result<Self>;
    pub fn list_sessions(&self, uid: &str, filter:&Filter) -> Result<Vec<SessionRecord>>; // deleted_at IS NULL AND user_id=?
    pub fn get_session(&self, id:&str) -> Result<Option<SessionRecord>>;
    pub fn insert_copied(&self, src:&SessionRecord, target_uid:&str, new_id:&str) -> Result<()>; // 21 列参数化 INSERT
    pub fn update_session_meta(&self, id:&str, patch) -> Result<()>;
}
```

- `SessionRecord` 字段严格对应 §2.3 的 21 列，`#[derive(Serialize, Deserialize, Clone)]`。
- **全部 SQL 参数化**（`?` 占位），禁止字符串拼接 id/uid（WorkDaddy 专门做了标识符白名单校验，Rust 侧用参数化从根本上规避注入）。
- 会话 id 合法性校验：UUID 格式白名单（`is_valid_session_id`），拒绝异常输入。
- 打开连接时使用 `busy_timeout`，并以只读/读写两种模式区分（浏览用只读，迁移用读写事务，整批复制在一个事务里提交，失败回滚）。

### 5.2 `session_files.rs`：5 类文件迁移

```rust
pub fn collect(data_root:&Path, id:&str) -> Result<Vec<SessionFile>>;      // 收集 5 类，拒绝符号链接
pub fn copy_remap(data_root:&Path, old_id:&str, new_id:&str, skip:&HashSet<String>) -> Result<CopyReport>;
pub fn delete_session_files(data_root:&Path, id:&str) -> Result<()>;       // 回滚用
fn remap_rel_path(rel:&str, old:&str, new:&str) -> Result<String>;         // 路径段精确替换
fn rewrite_artifact_owner(file:&Path, new_id:&str) -> Result<()>;          // _meta.ownerConversationId
```

路径规则（与 WorkDaddy `remapSessionArchivePath` 完全一致，避免误伤同名子串）：

- 只在**规定的路径段**替换 id：`projects/*/<id>.jsonl`、`projects/*/<id>/...`、`workspace/sessions/<id>/...`、`tasks/<id>/...`、`file-history/<id>/...`、`artifact-index/<id>.json`。
- 复制时：跳过符号链接；目标父目录逐级 `create_dir_all` 且校验路径上无符号链接/同名文件占位；目标已存在时按"更新"语义覆盖（lineage 对账场景），新建场景用"创建新文件"语义。
- `artifact-index/<id>.json` 复制后解析 JSON，把 `_meta.ownerConversationId`（以及内部引用 old id 的字段）改成 newId 再写回。
- 返回 `CopyReport{copied, failed, failed_files:[...]}`，失败文件不阻断整体但计入 partial。

### 5.3 `session_lineage.rs`：跨账号副本关系（持久化到本应用数据目录）

新增数据文件 `%APPDATA%\TraeWorkAssistant\data\wb_session_lineage.json`：

```json
{
  "version": 1,
  "lineages": {
    "<lineageId>": {
      "createdAt": 0,
      "members": [ {"uid":"A","sessionId":"uuid-A"}, {"uid":"B","sessionId":"uuid-B"} ],
      "mapping": { "B": {"targetId":"uuid-B","status":"copied","failedFiles":0} }
    }
  }
}
```

- 首次把账号 A 的会话复制到 B 时：为源会话分配 `lineageId=uuid`，登记 A、B 两个成员。
- 之后任何方向切换都凭 lineage 找到对方已有副本，**只更新文件/元数据，不再 INSERT**，根治重复会话。
- `pick_latest_member(data_root, lineage)`：比较各成员 `projects/**/<id>.jsonl` 等文件的最大 mtime，选最新者为同步源（等价 `syncAutoCopyLineage`）。
- 用 `(lineageId, targetUid)` 维度的内存 `Mutex<HashMap<_, JoinHandle/()>>` 串行化，复用本项目 `safe_lock` 毒化恢复模式。

### 5.4 `session_jobs.rs`：自动复制队列与进度

```rust
pub struct SessionJob { pub id:String, source_uid:String, target_uid:String,
    pub total:usize, pub processed:usize, pub copied:usize, pub skipped:usize,
    pub partial:usize, pub failed:usize, pub status:JobStatus }
```

- 全局**单条串行队列**（`Mutex<VecDeque<Job>>` + 一个 worker 线程），保证 A→B→C 快速连切时后一个任务能看到前一个任务创建的行（WorkDaddy 的关键经验）。
- 任务执行：① 重新 `list_sessions` 规划（不在切换响应里预扫描，避免卡切换界面）；② 每个会话之间 `tokio::task::yield_now().await` 让出；③ 有 lineage 先对账、缺失才新建；④ 每处理一个就 `app.emit("wb-session-job", &job)`。
- 复制规则持久化到 `data/wb_migrate_rules.json`：`{ "<uid>": { allSessions:bool, sessionIds:[], workspaces:[] } }`，前端可配。
- job 完成后保留 30 分钟供前端查询，然后清理。

### 5.5 迁移与切换的事务/失败语义

- 单个会话：先 `insert_copied`，再 `copy_remap`；文件阶段失败则反向 `delete_session_files(newId)` + `DELETE FROM sessions WHERE id=newId`，保证不留半截记录。
- 整批：数据库写入包在事务中；文件复制失败只标记该会话 partial，不影响其它会话。
- 目标账号下若已存在同 lineage 会话 → 走更新，状态记 `skipped`。

---

## 6. 模块详细设计 — 能力 C：WorkBuddy 加密归档导入导出

### 6.1 `session_transfer.rs`（移植 `.wds v4`，Rust 原生实现）

**新增依赖**：`scrypt`、`aes-gcm`、`flate2`（均为纯 Rust，无系统依赖）。

**字节布局（与 WorkDaddy 互通，便于生态兼容）：**

```
偏移  大小  字段
0     8    MAGIC = b"WDS4\r\n\x1a\n"
8     16   salt
24    12   nonce/iv
36    16   AES-256-GCM tag
52    ..   ciphertext = AES_256_GCM( key=scrypt(pwd,salt), aad=header[0..36], plaintext=gzip(帧流) )

帧流：重复 [ u32BE len ][ len 字节 JSON ]
- archive 帧：{ "type":"archive", "format":"wds", "version":4, "createdAt":.., "count":N }
- session 帧：{ "type":"session", "record": <21列 SessionRecord>, "fileCount":K }
- file    帧：{ "type":"file", "path":"projects/x/uuid.jsonl", "size":S, "data": <原始字节> }
```

> Rust 实现差异：WorkDaddy 为流式把文件字节直接进 gzip；Rust 侧用 `flate2::write::GzEncoder` + `aes_gcm::Encryptor` 组合成 `Write` 管道，同样边读边压边加密，内存占用恒定。解密反向：先把**全部密文喂给 GCM 并在 `finalize` 验证 tag 通过后**才开始 gunzip（WorkDaddy 强调"先认证后解密/解压"，防止恶意压缩炸弹与篡改）。

**导出 `wb_session_export(ids, password, out_path)`：**

1. 校验密码强度（≥10 位且含两类字符，复用 `requiredPassword` 规则）。
2. 从 db 取 21 列 record，`session_files::collect` 收集文件（拒绝符号链接、单会话 ≤20000、总数 ≤100）。
3. 写 archive/session/file 帧 → gzip → GCM → 落盘；返回路径与数量。

**导入 `wb_session_import(file_bytes, password, target_uid?)`：**

1. 校验 MAGIC、scrypt 派生密钥、GCM tag 验证（失败立即报错，不解压）。
2. gunzip 逐帧解析；对每个 file.path 跑 `remap_rel_path(oldId→newId)`，拒绝穿越/绝对路径/重复路径。
3. 每会话生成新 uuid，`target_uid` 显式指定时覆盖归属，否则用 record 原 `user_id`（并校验非空、无控制字符）。
4. 文件先落临时暂存目录，全部校验通过后在一个 db 事务里 `insert_copied`，再把暂存文件移动到正式路径（独占创建、0600）；任一失败回滚并清理。

### 6.2 命令

| 命令 | 说明 |
|---|---|
| `wb_session_list` | 列出当前账号（可指定 uid）会话：id/title/cwd/updated_at/文件大小，供迁移与导出选择 |
| `wb_session_export` | 入参 ids + password + 走系统保存对话框（`tauri-plugin-dialog`，已引入）选路径 |
| `wb_session_import` | 读文件 + password + 可选 targetUid，返回 `{imported:[{sourceId,id,uid}], failed, errors}` |
| `wb_migrate_rules_get/set` | 读写自动复制规则 |
| `wb_session_job_status` | 查询 job 进度；进度同时经 `wb-session-job` 事件推送 |

---

## 7. 模块详细设计 — 能力 D：Trae 对话解密与导出（P1，只读）

### 7.1 新增依赖（Cargo.toml）

```toml
# 对称加密 / 派生（能力 D/E/C 共用）
aes = "0.8"
cbc = { version = "0.1", features = ["alloc"] }
ctr = "0.9"                 # SQLCipher 实际是 CBC，保留 ctr 以备其它格式
hmac = "0.12"
pbkdf2 = { version = "0.12", default-features = false }
sha2 = "0.10"               # 已存在
md-5 = "0.5"                # 能力 E：telemetry.machineId = md5(machineid)
scrypt = "0.11"             # 能力 C
aes-gcm = "0.10"            # 能力 C
flate2 = "1"                # 能力 C gzip
rusqlite = { version = "0.31", features = ["bundled"] }   # 能力 B/D 明文库访问
# Windows 进程内存读取（能力 D 密钥扫描）
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_Foundation","Win32_System_Diagnostics_ToolHelp","Win32_System_ProcessStatus","Win32_System_Threading"] }
```

> 只读导出（D）**不需要** SQLCipher native 驱动：页面级解密产出明文 db 字节后，用 `rusqlite::Connection::open_in_memory()` + `sqlite3_deserialize`（rusqlite 的 `load_extension`/deserialize  blob）或直接把明文字节写临时 `.db` 再打开。后者实现最简单，临时文件用完删除，作为首选。

### 7.2 `trae_session/paths.rs`

- 复用/对齐 `switch.rs::tr_find_trae_dirs()` 的 4 个候选目录名，补一个 `ModularData/ai-agent/database.db` 定位函数：
  `fn locate_db(product: Product) -> Option<PathBuf>`，返回 (产品名, db 路径, 是否运行中)。
- 支持产品枚举 `SoloCn / TraeCn`，前端可切换；自动探测两个都存在的情况。

### 7.3 `mem_scan.rs`（Windows）

- 枚举进程（`CreateToolhelp32Snapshot/Process32First/Next`）匹配 `TRAE SOLO CN.exe`、`Trae CN.exe`。
- `OpenProcess(PROCESS_QUERY_INFORMATION|PROCESS_VM_READ)` → `VirtualQueryEx` 遍历可读内存区域 → `ReadProcessMemory` 分块（如 16 MiB/块，跨块边界保留 66 字节重叠避免截断密钥）。
- 正则匹配 `x'[0-9a-f]{64}'`，收集候选 32 字节密钥；逐候选用 §2.1 的首页 HMAC 校验，返回第一个通过的。
- 降级路径：扫描失败（权限/进程未运行）时，允许用户**手动提供密钥**或选择一份**已解密的明文 db**，保证 IDE 没开也能导出历史库（前提是用户曾导出过密钥）。

### 7.4 `decrypt.rs`

```rust
pub fn derive_key(raw:&[u8;32], salt:&[u8;16]) -> [u8;32];  // PBKDF2_HMAC_SHA512 x256000
pub fn page_hmac_key(key:&[u8;32]) -> [u8;64];              // SHA512(key)
pub fn verify_first_page(db:&[u8], raw:&[u8;32]) -> bool;   // 首页 HMAC 校验
pub fn decrypt_database(db:Vec<u8>, raw:&[u8;32]) -> Result<Vec<u8>>; // 逐页 AES-CBC 解密拼明文 SQLite
```

常量：`PAGE=4096, RESERVE=80, IV_LEN=16, ITER=256000`；页号小端；首页保留 SQLite 头（前 100 字节是 `"SQLite format 2000\0..."`，加密首页从偏移 16 开始，解密后需还原标准头——以 trae-session-export 的 `decrypt_db.py` 为准逐行移植，落地时以其边界处理为基准写单元测试）。

### 7.5 `schema.rs / rebuild.rs / export_md.rs`

- `schema.rs`：6 张表的结构体 + 预处理语句；`content/messages` 字段是 JSON 字符串，用 serde_json 解析。
- `rebuild.rs`：
  - `load_session_messages(conn, session_id) -> Vec<Turn>`：先尝试 `server_history_info` 流重组（按时间戳排序、展开 `raw_messages`、合并增量），数据缺失再回退 `history_v2 + general + task.summary`。
  - `Turn { role, text, tool_calls:Vec<ToolCall> }`，`ToolCall{name, params_json, result?}`。
- `export_md.rs`：单会话 Markdown（标题、时间、user/assistant、工具调用折叠块，风格对齐 trae-session-export）；`export_json` 导出结构化 JSON 供二次处理。
- 文件命名：`safe(标题)_<session_id 前8>.md`，输出目录由用户选（dialog）或默认 `文档/Trae会话导出/<产品>/<日期>/`。

### 7.6 命令（`commands/trae_session.rs`）

| 命令 | 说明 |
|---|---|
| `trae_db_probe` | 探测已安装产品、db 路径、IDE 是否运行、db 大小/修改时间 |
| `trae_session_list` | 入参 product（+可选 raw_key）；内部解密到临时库，返回会话列表（不向前端回传密钥） |
| `trae_session_export` | 入参 `{product, ids, format, out_dir, raw_key?}`，导出并返回文件清单 |
| `trae_session_drop`（P3，能力 G） | 用 SQLCipher 驱动写回，删除指定会话（见 §9 风险，默认隐藏/实验开关） |

---

## 8. 能力 E/F（P2）与能力 G（P3）路线

### 8.1 E：Trae storage.json 原生加密写入（Rust）

- 在 `trae_session/crypto_write.rs` 用 `aes+cbc+hmac+sha2` 1:1 移植 §2.2 的 `iCubeAuthInfo` 加解密（含 `AES_CONSTANT = JQ XOR WQ` 两个常量数组、header、内嵌 key、明文前缀 SHA512 自校验）。
- 新增 `trae_native_switch(account, {keep_state_vscdb:true})`：定位 `User/globalStorage/storage.json` → 改键 → 写 machineid → md5 telemetry → 保留 state.vscdb → 重启。
- 设置页增加"Trae 切换引擎：PowerShell 快照（默认，稳定）/ Rust 原生（快速，实验）"。先以"能对同一份 storage.json 加解密互逆"为验收（单元测试：加密→解密还原 JSON），再灰度。

### 8.2 F：多开实例

- `trae_instance_launch(dir_id)`：以 `%APPDATA%\TraeWorkAssistant\data\trae_instances\<id>` 为 `--user-data-dir`、共享 `--extensions-dir`，spawn 产品 exe。
- 实例内独立登录态，配合 E 的原生写入实现"每实例固定一个账号、多账号同时在线"。此能力依赖 E，列 P2。

### 8.3 G：Trae 跨账号会话迁移（实验性 POC）

- 技术路径：内存取密钥 → 页面级解密 → 在明文库 `UPDATE/复制` 会话并改归属 → 用 SQLCipher **重新加密**写回（这一步必须引入 SQLCipher 写能力，`rusqlite`  bundled 不含加密，需 `rusqlite` + `sqlcipher` 或捆绑官方 amalgamation，编译成本高，故列 P3）。
- **不确定性**：Trae 会话与服务端 `conversation_id`/账号绑定，改本地库后客户端能否展示、是否被服务端回灌覆盖，均需实测。结论：**D 先交付确定价值；G 单独立项做 POC，不承诺可用、默认不在正式界面暴露。**

---

## 9. 前端设计

### 9.1 新增/改动页面

1. **WorkBuddy 账号页（改 `WorkBuddyAccounts.tsx`）**
   - 每个账号行新增"切换到该账号（客户端登录）"主操作；切换确认弹窗提供"会话跟随"选项：自动迁移全部 / 每次询问 / 不迁移（对应 rules）。
   - 顶部新增"备份当前 WorkBuddy 登录"按钮（对应 `wb_auth_backup_current`）。
   - 新增"会话管理"抽屉：会话列表（标题/工作区/时间/大小）、多选、导出 `.wds`、导入 `.wds`、迁移规则设置。
2. **新增 Trae 对话存档页（`src/pages/TraeSessions.tsx`）**，侧边栏（`Sidebar.tsx`）加入口：
   - 产品选择（SOLO/Trae CN）、状态提示（IDE 运行中可自动取密钥 / 未运行需手动）。
   - 会话列表（搜索、按时间分组、全选）、导出格式（MD/JSON）、导出目录选择、导出进度与结果。
3. **设置页（`Settings.tsx`）**：Trae 切换引擎选择（P2）、迁移默认策略、导出默认目录、归档密码强度提示。

### 9.2 `src/lib/tauri.ts` 增量（集中封装，沿用现有风格）

```ts
workbuddy: {
  // ...既有
  authList: () => invoke<WbStoredAuth[]>('wb_auth_list'),
  backupCurrentAuth: () => invoke<{uid:string}>('wb_auth_backup_current'),
  switchAccount: (uid:string, opts:{reload?:boolean; migrate?:'auto'|'ask'|'off'}) =>
    invoke<WbSwitchResult>('wb_switch_account', { uid, opts }),
  sessionList: (uid?:string) => invoke<WbSession[]>('wb_session_list', { uid }),
  sessionExport: (ids:string[], password:string) => invoke<WbExportResult>('wb_session_export',{ids,password}),
  sessionImport: (password:string, targetUid?:string) => invoke<WbImportResult>('wb_session_import',{password,targetUid}),
  migrateRulesGet/set, jobStatus,
},
traeSession: {
  probe: () => invoke<TraeDbProbe>('trae_db_probe'),
  list: (product:string, rawKey?:string) => invoke<TraeSession[]>('trae_session_list',{product, rawKey}),
  export: (args:TraeExportArgs) => invoke<TraeExportResult>('trae_session_export',{args}),
},
```

新增事件（在 `setupListeners` 的 `ListenerHandlers` 中登记，模式同 `onCheckinProgress`）：

- `wb-session-job`：迁移任务进度（total/processed/copied/skipped/partial/failed/status）。
- `wb-switch-progress/done`：客户端切换与重启进度。
- `trae-export-progress/done`：解密与导出进度（解密大库可能耗时数秒）。

### 9.3 `src/types.ts` 增量

新增 `WbStoredAuth / WbSession / WbSessionJob / WbMigrateRules / TraeDbProbe / TraeSession / TraeExportResult` 等接口，字段与 Rust serde 输出 snake_case 对齐（本项目现有约定）。

### 9.4 状态管理（Zustand `store.ts`）

- 新增 wbSession 切片（会话列表、job 进度、规则）与 traeSession 切片（探测结果、会话列表、导出状态）。
- job 进度由事件驱动更新 store，组件订阅；切换完成后刷新 WorkBuddy 账号与会话列表。

---

## 10. 数据与存储布局（新增项汇总）

全部落在既有 `%APPDATA%\TraeWorkAssistant\data\` 下，不污染官方目录：

```
data/
├── wb_auth/<uid>.info                 # 【A】WorkBuddy 官方登录文件备份（原样，0600）
├── wb_session_lineage.json            # 【B】lineage 成员/映射
├── wb_migrate_rules.json              # 【B】自动复制规则
├── trae_instances/<id>/...            # 【F】Trae 多开 user-data-dir
└── （临时）trae_plain_<rand>.db        # 【D】解密临时库，导出后立即删除，不长期保留
```

官方目录**只在必要时被写入**：A 写 WorkBuddy auth 文件；B 写 `~/.workbuddy/workbuddy.db` 与 5 类会话文件；D 对 Trae 库**只读**；E/G 才写 Trae 目录（P2/P3）。

---

## 11. 安全设计（把 WorkDaddy 的防护清单作为硬性规范）

1. **路径安全**：所有归档/迁移文件路径必须经 `remap` 白名单校验，拒绝 `..`、绝对路径、盘符、符号链接（`lstat` 判断）、重复目标；父目录逐级校验非符号链接。
2. **SQL 安全**：一律参数化；session id 走 UUID 白名单正则；不把外部输入拼进 SQL/文件路径。
3. **加密归档**：先 GCM 认证全部密文、tag 通过后才解压（防压缩炸弹/篡改）；限制会话数、文件数、元数据帧大小；解压总量设上限。
4. **原子写**：登录文件、db 外文件均 tmp+rename / 独占创建；权限 0600；失败回滚（删半成品文件 + 回滚事务）。
5. **密钥卫生**：Trae 内存密钥只在后端使用、不写日志、不回传前端、不落盘；临时明文 db 用完即删（可选安全删除覆写）。归档密码不落盘、不进日志。
6. **token 脱敏**：前端只收 meta，沿用 `account_meta` 剥离 access/refresh token。
7. **操作可观测**：所有切换/迁移/导出写 `logs/`（switcher.log / 新增 session.log），记录 uid、数量、结果，不记录敏感内容。
8. **风控提示**：迁移/多开属非官方能力，UI 明确"请控制在个人使用范围、操作前建议先备份"，与现有签到风控免责一致。

---

## 12. 分阶段实施计划

| 阶段 | 内容 | 依赖 | 验收 |
|---|---|---|---|
| M1（P0） | 能力 A：WB 登录文件备份/原子写回/客户端重启切换；前端账号行"切换" | `auth_file.rs` 扩展 | 能在两个 WB 账号间切换客户端登录，文件不损坏、可回切 |
| M2（P0） | 能力 B：rusqlite + 5 类文件复制 + 单会话迁移 + 切号自动队列 + lineage 去重 | M1 | 切号后源账号对话出现在目标账号；来回切不重复、不丢失；进度可见 |
| M3（P1） | 能力 C：`.wds` 加密归档导入导出（Rust scrypt+GCM+gzip） | M2 | 导出包加密可在另一台设备导入并归属指定账号；恶意路径/错误密码被拒 |
| M4（P1） | 能力 D：Trae 内存取密钥 + 页面级解密 + 会话浏览 + MD/JSON 导出 | 新增加密/内存依赖 | IDE 运行时可解密列出会话并导出完整对话（含工具调用）；只读不写 |
| M5（P2） | 能力 E：storage.json 原生加解密互逆 + 原生切换；能力 F 多开 | M4 | 加解密单测互逆；原生切换可登录、保留设置；多实例可同时在线 |
| M6（P3） | 能力 G：Trae 跨账号迁移 POC（SQLCipher 写回） | M5 + SQLCipher 写能力 | 仅出实验报告/隐藏入口，按实测结果决定是否产品化 |

每个 M 都遵循本项目既有闭环：Rust 命令 → `main.rs` 注册 → `tauri.ts` 封装 → `types.ts` 类型 → 页面/组件 → 事件进度；后端为纯逻辑补 `#[cfg(test)]` 单测（参照 `workbuddy/accounts.rs` 现有测试密度，路径 remap、加解密互逆、lineage 去重、归档解包必须有单测）。

---

## 13. 风险与应对

| 风险 | 等级 | 说明与应对 |
|---|---|---|
| WorkBuddy 升级改库表/文件布局 | 中高 | 会话列与路径以常量集中管理（`SESSION_COPY_COLUMNS`、5 类路径），启动时做 schema 探测（`PRAGMA table_info`），列缺失则降级并提示版本不兼容，不硬写 |
| 迁移时客户端正在写 workbuddy.db | 中 | 切换/迁移前检测 WorkBuddy 进程，建议先退出或走重启切换；连接加 `busy_timeout`，整批事务，失败可重试 |
| lineage 长期积累脏数据/重复 | 中 | 每次任务前 `normalize`（校验成员会话仍存在、user_id 正确），规范选主；提供"重建/清理 lineage"入口 |
| Trae 内存扫描被安全软件拦截或失败 | 中 | 降级为手动密钥/已解密库；明确提示需要 IDE 运行与读取权限；不因此阻断导出 |
| SQLCipher 参数随 Trae 版本变化 | 中 | 参数集中常量；`trae_db_probe` 先做首页 HMAC 自检，失败给出"版本可能不兼容"而非乱码 |
| 引入 rusqlite bundled 增大体积/编译时间 | 低 | bundled 静态链接，体积增量可接受；如需要可后续换动态库 |
| 能力 E/G 写官方目录导致登录异常 | 高（仅 P2/P3） | 默认关闭、实验开关、操作前自动备份原 storage/db；G 不承诺可用 |
| 多账号风控 | 中 | 沿用既有免责声明与频率约束，迁移为本地操作不发起网络请求 |

---

## 14. 验收标准（汇总）

**A 切换**：两个 WB 账号可来回切换；官方 auth 文件始终为合法 JSON 且 uid 与所选一致；中途失败不损坏原文件（备份可恢复）。

**B 迁移**：切号后目标账号可见源对话（列表 + 点得开 + 产物可见）；消息正文、任务、文件历史、产物不缺；A→B→A 不产生重复会话；job 进度数字准确，失败会话有原因且不影响其它。

**C 归档**：导出包离开本机、用正确密码可导入并归属到指定 uid；错误密码/篡改包/路径穿越包被安全拒绝；大附件导出内存恒定不爆。

**D 导出**：与 trae-session-export 对同一 db 的导出结果在会话数、消息条数、工具调用上一致（作为对拍基线）；导出过程不修改原始加密库。

**E/F（P2）**：storage.json 加密→解密还原一致（单测）；原生切换与多开可用且保留 IDE 设置。

---

## 附录 A：关键路径速查（Windows）

```
# WorkBuddy
登录文件  %LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info
数据根    %USERPROFILE%\.workbuddy
会话库    %USERPROFILE%\.workbuddy\workbuddy.db                 (表 sessions，明文 SQLite)
消息      %USERPROFILE%\.workbuddy\projects\<hash>\<sessionId>.jsonl
附属      ...\projects\<hash>\<sessionId>\
          ...\workspace\sessions\<sessionId>\
          ...\tasks\<sessionId>\
          ...\file-history\<sessionId>\
产物索引  ...\artifact-index\<sessionId>.json   (改 _meta.ownerConversationId)

# Trae
会话库    %APPDATA%\TRAE SOLO CN\ModularData\ai-agent\database.db   (SQLCipher4 加密)
          %APPDATA%\Trae CN\ModularData\ai-agent\database.db
登录态    <产品目录>\User\globalStorage\storage.json   (iCubeAuthInfo://icube.cloudide 加密)
机器码    <产品目录>\machineid
IDE 设置 <产品目录>\User\globalStorage\state.vscdb      (切换时保留)
```

## 附录 B：加密参数速查

| 项 | Trae 会话库 | Trae iCubeAuthInfo | WB .wds 归档 |
|---|---|---|---|
| 算法 | AES-256-CBC | AES-128-CBC | AES-256-GCM |
| 密钥派生 | PBKDF2-HMAC-SHA512 ×256000 | SHA512 双层（内嵌随机 key） | scrypt(password,salt) |
| 完整性 | HMAC-SHA512/reserve 80 | 明文前缀 SHA512 自校验 | GCM tag（AAD=头36字节） |
| 分页/封装 | 页 4096，IV16+HMAC64 | base64(header6+key32+密文) | 52字节头 + gzip 长度帧 |
| 密钥来源 | 进程内存 `x'<64hex>'` | 随机 embedded_key | 用户密码 |

## 附录 C：参考项目可直接复用/移植的文件清单

| 来源项目 | 文件 | 复用方式 |
|---|---|---|
| trae-session-export | `decrypt_tool/scan_solo.py`、`decrypt_db.py` | 逐行翻译为 Rust `mem_scan.rs/decrypt.rs` |
| trae-session-export | `trae_web.py`（会话查询/重建/工具渲染） | 翻译为 `schema.rs/rebuild.rs/export_md.rs` |
| Account-Manager | `machine.rs`（iCubeAuthInfo 加解密、storage 写入、多开启动） | 移植为 `crypto_write.rs` + 多开命令 |
| WorkDaddy | `scripts/session-db.js` | `session_db.rs`（换 rusqlite，保留参数化/事务思路） |
| WorkDaddy | `scripts/daemon.js`（collect/copy/insert/lineage/autoCopy 3507-4330 区段） | `session_files.rs/session_lineage.rs/session_jobs.rs` |
| WorkDaddy | `scripts/session-transfer.js`、`secure-transfer.js` | `session_transfer.rs`（.wds 格式与路径 remap） |
| WorkDaddy | `scripts/lib.js switchTo`、`profiles.js` | `auth_file.rs` 切换与路径常量 |
