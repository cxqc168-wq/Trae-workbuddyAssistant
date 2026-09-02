# 布局重构：模块胶囊切换 + 侧边栏分层 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入胶囊分段控制器（TRAE / WorkBuddy）做模块切换，WorkBuddy 三板块上提为侧边栏菜单，签到进度提升至全局 store，底部固定全局功能。

**Architecture:** store 新增 module 与 wbCheckin 状态；Sidebar 顶部胶囊（绝对定位滑块动画）+ 按模块渲染菜单 + 底部固定区；WorkBuddy.tsx（685 行）拆为三个独立页面；App.tsx 路由 case 调整。纯前端，Rust 零改动。

**Tech Stack:** React + TypeScript + zustand + Tailwind + lucide-react。

**关键参考（现有代码）：**
- `src/pages/WorkBuddy.tsx`——拆分源，三个板块逻辑都在里面（accounts/checkin/credits 三段）
- `src/store.ts`——zustand 模式：`create<AppState>()((set, get) => ({...}))`，行号 150 `view: 'dashboard'`、行号 277 `setView`
- `src/components/Sidebar.tsx`——现有 NAV 数组、激活样式 `bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900`、底部邀请按钮
- `src/types.ts:4-12`——现有 ViewKey 联合

---

### Task 1: 类型与 store 状态层

**Files:**
- Modify: `src/types.ts:4-12`（ViewKey）、`src/types.ts:210`（WorkBuddyViewKey）
- Modify: `src/store.ts`（module/wbCheckin/setModule/setWbCheckin）

- [ ] **Step 1: 修改 types.ts**

ViewKey 调整为：

```typescript
export type ViewKey =
  | 'dashboard'
  | 'accounts'
  | 'checkin'
  | 'credits'
  | 'logs'
  | 'api-service'
  | 'settings'
  | 'wb-accounts'
  | 'wb-checkin'
  | 'wb-credits';

export type ModuleKey = 'trae' | 'workbuddy';
```

删除 `WorkBuddyViewKey`（L210）——Task 3 拆分页面后不再需要（若 WorkBuddy.tsx 在本任务报引用错误，暂改为内联类型 `'accounts' | 'checkin' | 'credits'`，Task 3 一并删除）。

- [ ] **Step 2: store.ts 新增状态与 action**

在 `interface AppState` 中（`view: ViewKey;` 之后）追加：

```typescript
  module: ModuleKey;
  wbCheckin: WbCheckinState;
```

（在文件顶部 Toast 接口附近定义：）

```typescript
export interface WbCheckinState {
  running: boolean;
  results: WorkBuddyCheckinProgress[];
}
```

import type 中追加 `ModuleKey, WorkBuddyCheckinProgress`。

action 声明（`setView` 之后）：

```typescript
  setModule: (m: ModuleKey) => void;
  setWbCheckin: (partial: Partial<WbCheckinState>) => void;
```

初始值（`view: 'dashboard',` 之后）：

```typescript
  module: 'trae',
  wbCheckin: { running: false, results: [] },
```

action 实现（`setView` 实现之后）：

```typescript
  setModule: (m) =>
    set({
      module: m,
      view: m === 'trae' ? 'dashboard' : 'wb-accounts',
    }),
  setWbCheckin: (partial) =>
    set((s) => ({ wbCheckin: { ...s.wbCheckin, ...partial } })),
```

- [ ] **Step 3: 类型检查**

Run: `npx tsc --noEmit`
Expected: WorkBuddy.tsx 的 `WorkBuddyViewKey` 引用与 `'workbuddy'` case（App.tsx L30）报错——**本步骤预期有错误**，记录错误清单，Task 2/3 修复。若除这两处外还有报错，先修复至只剩这两类。

- [ ] **Step 4: 提交**

```bash
git add src/types.ts src/store.ts
git commit -m "feat(layout): ViewKey 拆分 wb-* 与 module/wbCheckin 状态"
```

---

### Task 2: Sidebar 胶囊与分模块菜单

**Files:**
- Modify: `src/components/Sidebar.tsx`（整体重构）

- [ ] **Step 1: 重写 Sidebar.tsx**

完整新结构（保留现有邀请按钮逻辑与样式，menu-item 样式沿用）：

```tsx
import {
  LayoutDashboard,
  Users,
  PlayCircle,
  Coins,
  ScrollText,
  Settings,
  Gift,
  Server,
  Bot,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-shell';
import { useAppStore } from '../store';
import { cn } from '../lib/cn';
import type { ModuleKey, ViewKey } from '../types';

export type { ViewKey };

const TRAE_NAV: { key: ViewKey; label: string; icon: typeof Users }[] = [
  { key: 'dashboard', label: '概览', icon: LayoutDashboard },
  { key: 'accounts', label: '账号管理', icon: Users },
  { key: 'checkin', label: '一键签到', icon: PlayCircle },
  { key: 'credits', label: '积分看板', icon: Coins },
  { key: 'api-service', label: 'API 服务', icon: Server },
];

const WB_NAV: { key: ViewKey; label: string; icon: typeof Users }[] = [
  { key: 'wb-accounts', label: '账号列表', icon: Users },
  { key: 'wb-checkin', label: '一键签到', icon: PlayCircle },
  { key: 'wb-credits', label: '积分概览', icon: Coins },
];

const GLOBAL_NAV: { key: ViewKey; label: string; icon: typeof Users }[] = [
  { key: 'logs', label: '系统日志', icon: ScrollText },
  { key: 'settings', label: '系统设置', icon: Settings },
];

const MODULES: { key: ModuleKey; label: string; icon: typeof Users }[] = [
  { key: 'trae', label: 'TRAE', icon: Bot },
  { key: 'workbuddy', label: 'WorkBuddy', icon: Bot },
];

export default function Sidebar({
  view,
  onNav,
}: {
  view: ViewKey;
  onNav: (v: ViewKey) => void;
}) {
  const module = useAppStore((s) => s.module);
  const setModule = useAppStore((s) => s.setModule);
  const pushToast = useAppStore((s) => s.pushToast);
  const moduleNav = module === 'trae' ? TRAE_NAV : WB_NAV;

  const openInvite = async () => {
    try {
      const r = await (await import('../lib/tauri')).api.misc.inviteLink();
      await open(r.url);
    } catch (e) {
      pushToast('error', `打开邀请链接失败：${String(e)}`);
    }
  };

  return (
    <aside className="flex w-52 shrink-0 flex-col border-r border-slate-200 bg-white dark:border-zinc-800 dark:bg-zinc-950">
      {/* 模块胶囊分段控制器 */}
      <div className="p-3 pb-2">
        <div className="relative flex rounded-full bg-slate-100 p-1 dark:bg-zinc-900">
          {/* 滑块：绝对定位，宽度一半，随模块平移 */}
          <span
            aria-hidden
            className={cn(
              'absolute inset-y-1 left-1 w-[calc(50%-4px)] rounded-full bg-zinc-900 transition-transform duration-300 ease-out dark:bg-zinc-100',
              module === 'workbuddy' && 'translate-x-full',
            )}
          />
          {MODULES.map((m) => (
            <button
              key={m.key}
              onClick={() => setModule(m.key)}
              className={cn(
                'relative z-10 flex flex-1 items-center justify-center gap-1.5 rounded-full py-1.5 text-xs font-semibold transition-colors',
                module === m.key
                  ? 'text-white dark:text-zinc-900'
                  : 'text-slate-500 hover:text-slate-700 dark:text-zinc-400 dark:hover:text-zinc-200',
              )}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>

      {/* 模块菜单 */}
      <nav className="flex-1 space-y-1 p-3">
        {moduleNav.map((item) => {
          const Icon = item.icon;
          const active = view === item.key;
          return (
            <button
              key={item.key}
              onClick={() => onNav(item.key)}
              className={cn(
                'flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition active:scale-[0.98]',
                active
                  ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
                  : 'text-slate-600 hover:bg-slate-100 dark:text-zinc-400 dark:hover:bg-zinc-800',
              )}
            >
              <Icon size={17} />
              {item.label}
            </button>
          );
        })}
      </nav>

      {/* 底部固定：全局功能 */}
      <div className="border-t border-slate-200 p-3 dark:border-zinc-800">
        <nav className="mb-2 space-y-1">
          {GLOBAL_NAV.map((item) => {
            const Icon = item.icon;
            const active = view === item.key;
            return (
              <button
                key={item.key}
                onClick={() => onNav(item.key)}
                className={cn(
                  'flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition active:scale-[0.98]',
                  active
                    ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
                    : 'text-slate-600 hover:bg-slate-100 dark:text-zinc-400 dark:hover:bg-zinc-800',
                )}
              >
                <Icon size={17} />
                {item.label}
              </button>
            );
          })}
        </nav>
        <button
          onClick={openInvite}
          className="flex w-full items-center justify-center gap-2 rounded-lg bg-gradient-to-r from-amber-500 to-amber-400 px-3 py-2 text-sm font-semibold text-white shadow-[0_8px_20px_-8px_rgba(245,158,11,0.5)] transition hover:from-amber-400 hover:to-amber-300 active:scale-[0.98]"
        >
          <Gift size={16} />
          邀请得 5000 积分
        </button>
      </div>
    </aside>
  );
}
```

注意：胶囊按钮内不使用 m.icon（两段均为文字标签更干净）；若 lint/tsc 报 MODULES 中 icon 未使用，删掉 icon 字段。

- [ ] **Step 2: 类型检查**

Run: `npx tsc --noEmit`
Expected: 仅剩 App.tsx 的 `'workbuddy'` case 与 WorkBuddy.tsx 内部引用错误（Task 3 修复）。

- [ ] **Step 3: 提交**

```bash
git add src/components/Sidebar.tsx
git commit -m "feat(layout): 侧边栏胶囊模块切换与分模块菜单"
```

---

### Task 3: WorkBuddy 拆分三页面 + App 路由

**Files:**
- Create: `src/pages/WorkBuddyAccounts.tsx`、`src/pages/WorkBuddyCheckin.tsx`、`src/pages/WorkBuddyCredits.tsx`
- Delete: `src/pages/WorkBuddy.tsx`
- Modify: `src/App.tsx:14,30`

- [ ] **Step 1: 从 WorkBuddy.tsx 提取三个页面**

先通读 `src/pages/WorkBuddy.tsx`（685 行），按板块拆分（**逻辑原样迁移，不改行为**）：

**WorkBuddyAccounts.tsx**：
- 迁移：accounts/reloading 状态、reload、importLocal、ManualAddModal（含表单状态）、deleteAccount、账号卡片 JSX（徽标逻辑 expiresIn/needsRelogin/checkedToday/hasRefreshToken）、空状态
- 顶部 PageHeader：标题「WorkBuddy 账号」副标题「导入本机或手动添加 WorkBuddy 账号」+ 右侧操作按钮（导入/手动添加）
- 账号列表 reload 在本页 useEffect 挂载时触发

**WorkBuddyCheckin.tsx**：
- 迁移：selected/skipChecked 状态、事件监听、startCheckinAll、进度条与结果列表 JSX、勾选列表
- **状态提升改造**：`checkinRunning`/`checkinResults` 改用 store：
  ```tsx
  const checkinRunning = useAppStore((s) => s.wbCheckin.running);
  const checkinResults = useAppStore((s) => s.wbCheckin.results);
  const setWbCheckin = useAppStore((s) => s.setWbCheckin);
  ```
  所有 `setCheckinResults(...)` → `setWbCheckin({ results: ... })`；`setCheckinRunning(x)` → `setWbCheckin({ running: x })`；开始签到时 `setWbCheckin({ running: true, results: [] })`
- 事件监听 useEffect 依赖保持稳定（reload/toast 均稳定引用），挂载即 listen——页面卸载时清理监听但 store 状态保留，回切重新 listen 接续后续事件
- PageHeader：标题「WorkBuddy 签到」

**WorkBuddyCredits.tsx**：
- 迁移：creditsData/creditsLoading 状态、refreshCredits、每账号卡片 JSX（额度/进度条/警示行/resources 明细）、空状态
- PageHeader：标题「WorkBuddy 积分」+ 右侧刷新按钮

三个页面均为默认导出的独立组件，共享逻辑（formatNum/formatTime 等工具函数）若 WorkBuddy.tsx 中有，复制到各自文件或提取到 `src/pages/wb-shared.ts`（按实际数量决定：≥2 处复用则提取）。

- [ ] **Step 2: 删除 WorkBuddy.tsx**

确认三个新文件迁移完整后删除原文件（`git rm src/pages/WorkBuddy.tsx` 或删除文件后 git add 记录）。

- [ ] **Step 3: App.tsx 路由调整**

```tsx
import WorkBuddyAccounts from './pages/WorkBuddyAccounts';
import WorkBuddyCheckin from './pages/WorkBuddyCheckin';
import WorkBuddyCredits from './pages/WorkBuddyCredits';
```

switch 中删除 `case 'workbuddy'`，追加：

```tsx
    case 'wb-accounts':
      return <WorkBuddyAccounts />;
    case 'wb-checkin':
      return <WorkBuddyCheckin />;
    case 'wb-credits':
      return <WorkBuddyCredits />;
```

- [ ] **Step 4: 类型检查**

Run: `npx tsc --noEmit`
Expected: 零错误（WorkBuddyViewKey 引用已随原文件删除；types.ts 中该类型定义一并删除）。

- [ ] **Step 5: 提交**

```bash
git add src/pages/WorkBuddyAccounts.tsx src/pages/WorkBuddyCheckin.tsx src/pages/WorkBuddyCredits.tsx src/App.tsx src/types.ts src/pages/wb-shared.ts 2>$null
git add -u src/pages/WorkBuddy.tsx
git commit -m "feat(layout): WorkBuddy 拆分三页面并接入新路由"
```

（PowerShell 下若无 wb-shared.ts 则去掉该路径；`git add -u` 记录删除。）

---

### Task 4: 内容区淡入过渡与验收

**Files:**
- Modify: `src/App.tsx:82`（内容区淡入）

- [ ] **Step 1: 内容区淡入**

App.tsx 中给内容 div 加 key 触发重挂载淡入：

```tsx
<div
  key={view}
  className="relative min-h-0 flex-1 overflow-auto p-5 animate-[fadeIn_120ms_ease-out]"
>
  {renderView(view)}
</div>
```

并在 `src/index.css`（或项目现有全局样式文件，先确认 tailwind 配置位置）追加：

```css
@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}
```

若项目 tailwind config 已有扩展动画机制则用其方式；目的只是切换时 120ms 淡入。

- [ ] **Step 2: 验证**

1. `npx tsc --noEmit` 零错误
2. `npm run tauri dev` 启动（dev 服务器可能已在跑，若 5173 被占用则先停旧进程）
3. 按验收清单逐项手动验证：
   - 胶囊切换 TRAE/WorkBuddy，菜单联动正确
   - 底部全局项恒显，日志/设置可跳转
   - WorkBuddy 三页功能正常（添加账号/签到/积分）
   - **签到运行中切到 TRAE 概览再切回，进度仍在并继续递增**（核心验收）
   - Trae 各页不受影响
   - 亮/暗主题切换无异常

- [ ] **Step 3: 提交**

```bash
git add src/App.tsx src/index.css
git commit -m "feat(layout): 内容区切换淡入过渡"
```
