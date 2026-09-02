# Trae Work Assistant

Windows 桌面端多账号签到与管理工具 · Tauri 2 + React 18 + Rust

> ⚠️ 本工具与 Trae Work 官方无任何关联，仅供学习研究。使用本工具可能违反 Trae Work 服务条款，风险自担。请仅管理本人合法持有的账号。

## 简介

Trae Work Assistant 是一款开源的 Windows 桌面工具，为 **Trae** 与 **WorkBuddy** 双平台提供多账号管理与自动签到能力。应用采用 Tauri 2 架构（React 18 前端 + Rust 后端），数据全部本地存储，内置本地 MITM 代理与 OpenAI 兼容 API 网关，让账号管理与积分追踪一站完成。

双模块设计，侧边栏一键切换：

- **TRAE 模块**：账号管理、OAuth 登录、一键签到、积分看板、本地代理、API 网关、定时任务

- **WorkBuddy 模块**：独立的概览看板、账号管理（auth 文件导入 / Token 管理）、批量签到（并发保护 + 新旧端点回退）、积分查询与排行

## 截图

![主界面](docs/images/main.png?v=2)

## 免责声明

> 本工具仅供学习研究和个人使用，使用者需自行承担一切风险与后果。

1. **非官方申明**：本工具与 Trae / TRAE Work 官方**无任何隶属、合作或关联关系**，系个人开源项目，不代表官方立场。
2. **使用风险**：使用本工具可能违反 Trae Work 的服务条款；由此产生的任何后果（包括但不限于账号封禁、积分清零/扣除、功能限制、数据异常等）均由使用者自行承担。
3. **责任范围**：本工具不对因使用（或无法使用）本工具所导致的任何直接、间接、附带或后果性损失负责。
4. **合规义务**：使用前请务必仔细阅读 Trae Work 的服务条款，并自行判断是否使用；请确保仅用于管理本人合法持有的账号，遵守所在地法律法规。
5. **作者免责**：本工具作者对任何因使用、误用或滥用本工具而引发的纠纷、争议或问题不承担任何责任。
6. **侵权处理**：若您是 Trae Work 官方且认为本工具侵犯了您的合法权益，请通过项目渠道联系作者，我们将在核实后及时下架处理。

**使用本工具即表示你已阅读、理解并同意上述全部免责声明。**

## 功能

- **账号管理**：多账号 JWT 录入/编辑/查看、OAuth 登录、分组管理、设备 ID 隔离

- **登录态切换**：保存当前登录态 → 恢复目标账号 → 启动，精准备份 9 类核心文件

- **一键签到**：批量签到、按分组/手动勾选、跳过已签/过期、实时进度

- **积分看板**：排行、三线趋势图（总数/获得/消耗）、今日新增统计

- **WorkBuddy 支持**：独立模块覆盖 WorkBuddy 账号管理、批量签到（每账号互斥锁 + 全局轮次锁并发保护、失败自动回退旧 API 端点、Token 刷新与 401 自动重试）、积分概览

- **本地代理**：MITM 代理自动捕获 JWT、注入独立设备 ID；**自动串联已有系统代理（VPN）作为上游**，开启代理后外网访问不受影响，停止时原样还原系统代理

- **API 网关**：内嵌 OpenAI 兼容 API 服务，账号池智能调度（积分过期感知 + 冷却状态机）

- **定时任务**：Windows 计划任务，后台自动签到

- **6 层设备标识重置**：machineid / storage.json 遥测 / aha.device / 注册表 MachineGuid / webview 追踪数据 / aha TinyStorage

- **快照管理**：查看/备份/恢复/删除账号登录态快照

- **暗色模式**：全界面暗色主题支持，图表动态适配

- **数据全部本地存储**，不上传任何服务器

## 开发

```powershell
npm install
npm run tauri dev      # 开发模式
npm run tauri build    # 打包（msi + nsis）
```

前置：Node.js 18+、Rust 1.75+、Python 3.9+、WebView2 Runtime、VS Build Tools (C++)

## 数据目录

```
%APPDATA%\TraeWorkAssistant\
├── conf/
│   └── app_settings.json        # 设置
├── data/
│   ├── checkin_accounts.json    # 账号 + JWT
│   ├── device_map.json          # 设备 ID 映射
│   ├── groups.json              # 分组
│   ├── credits_history.json     # 积分历史（签到明细）
│   ├── credits_daily.json       # 每日积分快照（三线趋势图数据源）
│   ├── remaining_credits.json   # 各账号剩余积分缓存
│   ├── account_cooldowns.json   # 签到错误冷却状态
│   ├── api_pool.json            # API 账号池配置
│   └── profiles/                # 登录态快照（按账号 ID 分目录）
│       ├── current_account.txt  # 当前活跃账号 ID
│       └── <user_id>/           # 各账号登录态备份
└── logs/                        # proxy / checkin / switcher / api / proxy-requests 日志
```

## 文档

- [更新日志](CHANGELOG.md) — 各版本变更记录

- [用户手册](docs/user-manual.md) — 功能说明与使用指南

- [API 文档](docs/api-doc.md) — Tauri 前后端命令契约

- [技术框架](docs/tech-framework.md) — 架构设计与数据模型

- [运行手册](docs/operation-manual.md) — 开发环境与部署指南

## 作者

**极泊Poles** — 本项目作者

- GitHub：[@cxqc168-wq](https://github.com/cxqc168-wq)

- QQ：203505609

如果这个工具帮到了你，欢迎给项目点一个 ⭐ Star，或扫描下方赞赏码请作者喝杯咖啡 ☕

![赞赏码](src/assets/reward-qr.png)

## License

MIT
