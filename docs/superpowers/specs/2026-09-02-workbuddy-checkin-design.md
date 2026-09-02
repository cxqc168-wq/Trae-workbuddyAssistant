# WorkBuddy 自动签到集成设计

日期：2026-09-02
状态：已获用户批准

## 1. 背景与目标

当前项目（Trae Work 助手，Tauri + React）仅支持 Trae 账号自动签到。本设计将
`workbuddy-switch-main` 样例项目中 WorkBuddy（CodeBuddy）的核心能力移植进来，使
本项目也能对 WorkBuddy 账号执行自动签到。

移植范围（用户确认）：

1. 签到 + 状态查询
2. Token 自动刷新/保活
3. 本地账号导入（读 WorkBuddy 客户端认证文件）
4. 手动添加账号（粘贴 token）
5. 积分/额度查询

不移植：账号切换写入认证文件、进程管理、用量统计历史、导出导入、OAuth 采集、
Windows settings env 同步等样例中的其它模块。

## 2. 方案选型

**方案 A（采纳）：纯 Rust 原生移植。** 样例 `wb-switch-core` 的核心模块移植为
本项目 `src-tauri/src/workbuddy/` 模块，复用项目现有 `reqwest`/`tokio` 依赖与
`AppState` 数据目录体系，通过 Tauri command 暴露给前端。

- 账号数据独立存放于 `data_dir/workbuddy_accounts.json`，与 Trae 账号体系
  （`accounts.json`）完全隔离，互不干扰。

- 不复用样例的 `~/.wb-switch` 目录约定。

被否决的备选：B（Python 脚本，链路割裂）、C（直接依赖 wb-switch-core crate，
目录约定耦合严重）。

## 3. 模块划分（Rust 端）

```
src-tauri/src/workbuddy/
├── mod.rs        # 模块入口
├── accounts.rs   # 账号存储与脱敏视图
├── auth_file.rs  # 本地认证文件读取与导入
├── checkin.rs    # 签到流程与日志
├── refresh.rs    # token 刷新与惰性刷新
└── credits.rs    # 积分/额度查询
```

命令层：`src-tauri/src/commands/workbuddy.rs` 集中注册 Tauri commands。

### 3.1 accounts.rs

- 存储文件：`data_dir/workbuddy_accounts.json`，JSON 数组，原子写。

- 账号字段（对齐样例）：`id`(uuid)、`uid`、`nickname`、`email`、
  `enterpriseName`、`enterpriseId`、`access_token`、`refresh_token`、
  `token_type`、`domain`、`expiresAt`、`refreshExpiresAt`、`refreshedAt`、
  `createdAt`、`needs_relogin`、`needs_relogin_reason`。

- 去重合并：按 uid 优先，uid 缺失时按真实邮箱（含 `@` 且非占位值）兜底；
  命中已有身份保留本地 `id`，防止调用方引用失效。

- 脱敏视图 `account_meta`：不含 access\_token / refresh\_token。

### 3.2 auth\_file.rs

- 认证文件路径（Windows）：
  `%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info`
  （兼容 macOS/Linux 路径，同官方）。

- 解析四段 JSON（root/account/auth），字段提取链与样例一致：
  `accessToken` → `access_token` 等；时间戳字符串转 i64 不换算单位。

- 只读导入，不写认证文件。

### 3.3 refresh.rs

- 刷新端点：`POST https://www.codebuddy.cn/v2/plugin/auth/token/refresh`，
  请求头 `X-Refresh-Token: <refresh_token>`，体 `{}`。

- 成功（code 0/200）：更新 access/refresh token 与过期时间（`expiresIn`
  相对秒换算绝对毫秒），清除 needs\_relogin 标记并落盘。

- 失败：标记 `needs_relogin=true` + 原因，落盘，避免无限重试。

- 惰性刷新 `ensure_fresh_token`：`expiresAt` 缺失或剩余 < 24h 时刷新。

### 3.4 checkin.rs

- 状态查询：`POST /v2/billing/meter/checkin-activity-status`，失败回退
  `/checkin-status`；取 `data.today_checked_in|todayCheckedIn`。

- 执行签到：`POST /v2/billing/meter/daily-checkin`，体 `{}`；
  code 0/200 成功；消息含"已签到"/"repeat"按成功处理。

- 401/403 或消息含 unauthorized/token 等关键字且有 refresh\_token → 刷新
  一次后重试。

- 单账号流程：互斥锁 → 惰性刷新 → 查状态 → 未签则提交 → 写日志。

- 签到日志：`data_dir/logs/workbuddy_checkin.log`，条目含
  `result(ts/accountId/email/error)`；同时保留 `workbuddy_checkin_logs.json`
  供"今日已签"判定（移植样例 latest\_today\_result 逻辑）。

- 并发保护：全局轮次锁 + 单账号运行锁（静态 Mutex<HashSet>）。

### 3.5 credits.rs

- 新接口（并行三路）：`/billing/meter/get-user-resource-summary`、
  `-paid-packages`、`-free-packages`（注意：无 `/v2` 前缀，域名按账号
  domain 在 codebuddy.cn / workbuddy.cn 两个官方 origin 间选择）。

- 请求头附加 `X-Client-Platform: web`。

- 鉴权链路：惰性刷新 → 三路并行 → 任一 401 只刷新一次仅重试该分支。

- 解析多形态响应（Accounts/Packages 多层嵌套路径），汇总
  totalCapacity/totalRemaining/expiringSoon 等字段。

- 旧接口回退：`POST /v2/billing/meter/get-user-resource`；每轮查询至多一次
  401 刷新。

### 3.6 Tauri Commands

```
workbuddy_list_accounts() -> Vec<AccountMeta>
workbuddy_import_local() -> AccountMeta            # 本地认证文件导入
workbuddy_add_manual(token, refresh_token?, uid?, nickname?) -> AccountMeta
workbuddy_delete_account(account_id) -> ()
workbuddy_checkin_status(account_id) -> {todayCheckedIn}
workbuddy_checkin_all(ids?) -> 事件流              # 逐账号推送进度
workbuddy_credits(account_id?) -> Vec<CreditSummary>
workbuddy_refresh_token(account_id) -> AccountMeta
```

签到进度通过 `workbuddy-checkin-progress` / `workbuddy-checkin-done` 事件推送，
与现有 Trae 签到事件模式一致。

## 4. 前端设计

### 4.1 导航与页面结构

- `ViewKey` 新增 `'workbuddy'`，侧边栏新增「WorkBuddy」入口（图标
  `Bot`，置于"一键签到"之后）。

- 新页面 `src/pages/WorkBuddy.tsx`，内部三个板块：**账号列表 / 一键签到 /
  积分概览**。

### 4.2 板块切换交互（用户要求：按钮 + 左右滑动，非分页）

- 页面顶部按钮组（三个按钮，等宽），当前板块高亮。

- 主内容区：所有板块横向排列于 flex 容器，容器 `translateX(-index*100%)`

  - `transition` 实现左右滑动过渡；切换按钮索引决定方向（账号 0 / 签到 1 /
    积分 2，从左到右）。

- 保持挂载（不卸载非活动板块），保留各板块内部状态。

### 4.3 板块内容

- **账号列表**：账号卡片（昵称/邮箱、token 状态徽标：有效/临期/需重登、
  今日签到状态、删除按钮）；顶部操作：「导入本机账号」「手动添加」
  （弹窗表单：access\_token 必填，refresh\_token/uid/昵称选填）。

- **一键签到**：账号勾选列表、跳过已签开关、开始签到按钮、实时进度条与
  每账号结果（成功/已签/失败+原因）。

- **积分概览**：刷新按钮 + 每账号积分卡片（总额度、剩余、进度条、即将
  过期额度高亮）。

### 4.4 前端 API 封装

`src/lib/tauri.ts` 新增 `workbuddy` 命名空间，封装上述命令与事件监听。

## 5. 错误处理

- token 失效：自动刷新重试一次；刷新失败标记"需重新登录"，后续跳过。

- 网络失败：逐账号返回错误，不中断整轮。

- 本地导入失败：明确提示"未读取到本地 WorkBuddy 登录信息（需安装并登录
  WorkBuddy 客户端）"。

- 单账号并发签到互斥；全轮签到全局锁，重复触发返回 skipped。

## 6. 测试与验收

- Rust 单元测试：移植样例的关键测试（状态判定、去重合并、时间戳解析、
  401 判定、积分多形态解析、今日已签判定）。

- 手动验收：

  1. 本地导入：装有 WorkBuddy 客户端的机器导入成功，字段完整。
  2. 手动添加 token 后签到成功（真机验证）。
  3. 一键签到多账号：未签的签到、已签的跳过、失败带原因。
  4. 积分查询返回总额度/剩余。
  5. 板块按钮切换左右滑动动画正常。
  6. Trae 原有签到功能不受影响（数据文件隔离）。

## 7. 风险

- WorkBuddy 服务端接口变更：沿用样例的多形态解析与回退策略缓解。

- 认证文件格式随客户端升级变化：解析层多路径取值，缺失字段容忍。

- 手动添加的账号无 uid/refresh\_token：签到遇 401 无刷新能力，标记需重登。

