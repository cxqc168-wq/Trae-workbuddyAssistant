import { useEffect, useState } from 'react';
import { useAppStore } from '../store';

/** 手绘线性太阳图标：圆心 + 8 条射线，stroke 2 圆角端点 */
function SunIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.93 4.93l1.77 1.77M17.3 17.3l1.77 1.77M19.07 4.93l-1.77 1.77M6.7 17.3l-1.77 1.77" />
    </svg>
  );
}

/** 手绘线性月牙图标 */
function MoonIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z" />
    </svg>
  );
}

/**
 * 顶栏主题切换按钮：明 ↔ 暗两态循环（绕过「跟随系统」，点击即持久化）。
 * 当前生效主题 = settings.theme，为 system 时取系统偏好。
 */
export default function ThemeToggle() {
  const theme = useAppStore((s) => s.settings?.theme) ?? 'system';
  const saveSettings = useAppStore((s) => s.saveSettings);
  const [systemDark, setSystemDark] = useState(
    () => window.matchMedia('(prefers-color-scheme: dark)').matches,
  );

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);

  const isDark = theme === 'dark' || (theme === 'system' && systemDark);

  const toggle = () => {
    void saveSettings({ theme: isDark ? 'light' : 'dark' });
  };

  return (
    <button
      onClick={toggle}
      title={isDark ? '切换为明亮主题' : '切换为暗黑主题'}
      aria-label={isDark ? '切换为明亮主题' : '切换为暗黑主题'}
      className="btn h-8 w-8 shrink-0 !px-0 text-slate-500 hover:bg-slate-100 hover:text-slate-700 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
    >
      {/* 双图标交叉旋转缩放：充足动效但保持简约 */}
      <span className="relative block h-[18px] w-[18px]">
        <SunIcon
          className={`absolute inset-0 h-full w-full transition-all duration-300 ease-[cubic-bezier(0.34,1.56,0.64,1)] ${
            isDark ? 'scale-50 -rotate-90 opacity-0' : 'scale-100 rotate-0 opacity-100'
          }`}
        />
        <MoonIcon
          className={`absolute inset-0 h-full w-full transition-all duration-300 ease-[cubic-bezier(0.34,1.56,0.64,1)] ${
            isDark ? 'scale-100 rotate-0 opacity-100' : 'scale-50 rotate-90 opacity-0'
          }`}
        />
      </span>
    </button>
  );
}
