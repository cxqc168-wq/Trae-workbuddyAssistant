# 暗黑/明亮主题切换按钮设计

日期:2026-09-02
状态:已确认

## 背景

项目已具备双主题基础设施(Tailwind `darkMode: 'class'`、`settings.theme` 支持 system/light/dark、App.tsx 负责 class 切换与系统主题监听),Settings 页有下拉选择,但顶栏缺少快捷切换按钮。

## 需求

1. 顶栏添加主题切换按钮,图标为简约线性 SVG 设计
2. 全面审计现有页面在两种主题下的表现并打磨

## 已确认决策

- **切换行为**:两态循环(light ↔ dark),不引入 system 状态;Settings 页保留 system 选项
- **实施范围**:按钮 + 全面审计打磨(修复遗漏的 dark: 变体、对比度问题,不重构配色体系)
- **图标方案**:手绘线性 SVG(stroke-width 2,24×24 viewBox),亮色显示太阳、暗色显示月牙,旋转+缩放动效切换

## 设计

### 主题架构

复用现有体系,新增:

- **`src/components/ThemeToggle.tsx`**:读取当前生效主题,点击后调用 `saveSettings({ theme: 'light' | 'dark' })` 两态循环,立即持久化
- **放置位置**:`TopBar.tsx` 中 `TraeBar` 与 `WorkBuddyBar` 的右侧按钮组最左侧,两个模块统一可见

### 按钮视觉

- 32×32 圆角方形图标按钮(icon-only,`title` 提示)
- 线性 SVG:亮色显示太阳(圆心+8 条射线),暗色显示月牙
- 旋转+缩放过渡动画切换
- Hover:`bg-slate-100 dark:bg-zinc-800` 过渡,与现有 btn-outline 语言一致

### 审计范围

所有页面/组件:遗漏的 `dark:` 变体、对比度不足文字、边框/卡片/输入框层次一致性。保持 slate(亮)/ zinc(暗)设计语言。

## 验证

`tsc --noEmit` + `npm run tauri dev` 手动验证两种主题下各页面渲染。
