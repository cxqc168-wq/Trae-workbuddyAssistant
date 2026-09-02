# 软件布局重构：模块胶囊切换 + 侧边栏分层

日期：2026-09-02
状态：已获用户批准

## 1. 背景与目标

当前布局为扁平 8 项侧边栏（概览/账号/签到/WorkBuddy/积分/API/日志/设置），
WorkBuddy 入口只有一个，进入后页面内部还有三按钮切换（账号/签到/积分），
层级混乱且签到进度为页面本地态（切走丢失）。

目标：引入**胶囊状分段控制器**做模块切换（TRAE / WorkBuddy），模块在胶囊层、
功能项在侧边栏菜单层，底部固定全局功能；WorkBuddy 三板块上提为侧边栏菜单项；
签到进度提升至 store 不再丢失。保持经典左右双栏骨架。

## 2. 关键决策（用户确认）

| 决策点            | 选择             | 理由                     |
| -------------- | -------------- | ---------------------- |
| 胶囊位置           | B 侧边栏顶部（标题栏下方） | 层级最清晰，不碰窗口拖拽区          |
| WorkBuddy 菜单组织 | 三板块上提为侧边栏菜单项   | 与 TRAE 菜单同层级，侧边栏标明当前层级 |
| 全局功能（日志/设置）    | 固定侧边栏底部（分隔线下）  | 两模块下恒显                 |
| 右侧顶栏           | 全局不变           | 代理/API 本就是全局功能，实现最简    |

## 3. 模块划分

### 3.1 TRAE 模块菜单

- 概览（dashboard）

- 账号管理（accounts）

- 一键签到（checkin）

- 积分看板（credits）

- API 服务（api-service）

### 3.2 WorkBuddy 模块菜单

- 账号列表（wb-accounts）

- 一键签到（wb-checkin）

- 积分概览（wb-credits）

### 3.3 全局固定（底部）

- 系统日志（logs）

- 系统设置（settings）

- 邀请得积分按钮

## 4. 详细设计

### 4.1 状态层（src/store.ts）

新增：

```ts
module: 'trae' | 'workbuddy';
setModule: (m) => void;
wbCheckin: { running: boolean; results: WorkBuddyCheckinProgress[] };
setWbCheckin: (partial) => void;
```

- `setModule('trae')` 自动设 `view = 'dashboard'`；`setModule('workbuddy')` 设 `view = 'wb-accounts'`

- `ViewKey` 调整：移除 `'workbuddy'`，新增 `'wb-accounts' | 'wb-checkin' | 'wb-credits'`

- `wbCheckin` 状态：签到运行态与结果数组提升至全局 store，侧边栏切视图不丢失

### 4.2 Sidebar 重构（src/components/Sidebar.tsx）

结构（自上而下）：

1. **胶囊分段控制器**（顶部）

   - 两段等宽：TRAE / WorkBuddy

   - 容器 `rounded-full` + `bg-surface-deep` + `p-1`

   - 激活段：绝对定位滑块 `translateX(0% / 100%)` + `transition-transform duration-300 ease-out` + 白字深底

   - 非激活段：`text-muted`

   - 点击调 `setModule`
2. **模块菜单区**（flex-1）

   - 按 `module` 渲染对应菜单数组

   - 当前 view 高亮（沿用现有 `bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900`）
3. **分隔线**
4. **底部固定区**

   - 系统日志 / 系统设置（两模块下恒显，高亮逻辑同上）

   - 现有「邀请得积分」按钮

### 4.3 WorkBuddy 页面拆分

- 删除 `src/pages/WorkBuddy.tsx` 的三按钮 + translateX 滑动容器

- 拆为三个独立页面（逻辑原样迁移）：

  - `src/pages/WorkBuddyAccounts.tsx`：账号卡片列表 + 导入本机 + 手动添加弹层 + 删除

  - `src/pages/WorkBuddyCheckin.tsx`：账号勾选 + 跳过已签 + 开始签到 + 事件监听（读写 store.wbCheckin）+ 进度条 + 结果列表

  - `src/pages/WorkBuddyCredits.tsx`：刷新积分 + 每账号额度卡片 + resources 明细

- 各页独立挂载/卸载（与 Trae 各页行为一致），签到页挂载时从 store 恢复 running/results

### 4.4 App.tsx

`renderView` 调整：

```ts
case 'wb-accounts': return <WorkBuddyAccounts />;
case 'wb-checkin': return <WorkBuddyCheckin />;
case 'wb-credits': return <WorkBuddyCredits />;
```

删除 `case 'workbuddy'`。骨架（TitleBar / Sidebar / TopBar / 内容区）不变。

### 4.5 视觉细节

- 胶囊滑块：`absolute inset-y-1 left-1 w-[calc(50%-4px)]` + `translate-x-full` 切 workbuddy；`transition-transform duration-300 ease-out`

- 模块切换时内容区 `opacity` 淡入 120ms（CSS `transition-opacity`，非路由动画库）

- 顶栏 TopBar 全局不变

- 双主题（zinc/slate + dark:）沿用现有类名体系

## 5. 影响范围

纯前端重构，Rust 端零改动。涉及文件：

- 修改：`src/store.ts`、`src/types.ts`、`src/components/Sidebar.tsx`、`src/App.tsx`

- 新建：`src/pages/WorkBuddyAccounts.tsx`、`src/pages/WorkBuddyCheckin.tsx`、`src/pages/WorkBuddyCredits.tsx`

- 删除：`src/pages/WorkBuddy.tsx`（拆分后移除）

## 6. 验收标准

1. 胶囊切换 TRAE/WorkBuddy，下方菜单正确联动
2. 底部全局项（日志/设置/邀请）两模块下恒显
3. WorkBuddy 三页独立可用，各页功能与重构前一致
4. 签到运行中切到 Trae 概览再切回，进度仍在且继续递增
5. Trae 各页（概览/账号/签到/积分/API）不受影响
6. `npx tsc --noEmit` 零错误
7. 应用启动后布局正常，双主题切换无异常

## 7. 风险

- 签到事件监听器迁移到独立页面后，切走时 useEffect 清理会移除监听——需把"签到中"判定放在 store 层（running=true 时即使 Checkin 页卸载，后端仍在执行，回切时重新 listen 能接续后续事件）。已纳入设计 4.1/4.3。

- ViewKey 类型变更波及面广（Sidebar/App/store），需全量替换，遗漏会导致 tsc 报错（可控）。

