import { useEffect, useState } from 'react';
import TitleBar from './components/TitleBar';
import Sidebar from './components/Sidebar';
import TopBar from './components/TopBar';
import Toaster from './components/Toaster';
import { useAppStore } from './store';
import Dashboard from './pages/Dashboard';
import Accounts from './pages/Accounts';
import Checkin from './pages/Checkin';
import Credits from './pages/Credits';
import Logs from './pages/Logs';
import Settings from './pages/Settings';
import ApiService from './pages/ApiService';
import WorkBuddyDashboard from './pages/WorkBuddyDashboard';
import WorkBuddyAccounts from './pages/WorkBuddyAccounts';
import WorkBuddyCheckin from './pages/WorkBuddyCheckin';
import WorkBuddyCredits from './pages/WorkBuddyCredits';
import ActivationGate from './components/ActivationGate';
import { api, isTauri } from './lib/tauri';
import type { ViewKey } from './types';

function renderView(view: ViewKey) {
  switch (view) {
    case 'dashboard':
      return <Dashboard />;
    case 'accounts':
      return <Accounts />;
    case 'checkin':
      return <Checkin />;
    case 'credits':
      return <Credits />;
    case 'logs':
      return <Logs />;
    case 'api-service':
      return <ApiService />;
    case 'wb-dashboard':
      return <WorkBuddyDashboard />;
    case 'wb-accounts':
      return <WorkBuddyAccounts />;
    case 'wb-checkin':
      return <WorkBuddyCheckin />;
    case 'wb-credits':
      return <WorkBuddyCredits />;
    case 'settings':
      return <Settings />;
    default:
      return <Dashboard />;
  }
}

// 浏览器直接访问 Vite 开发服务器时展示的降级页：本应用为 Tauri 桌面程序，
// 所有功能依赖 Rust 后端，无法在纯浏览器环境运行。
function BrowserFallback() {
  return (
    <div className="flex h-full items-center justify-center bg-slate-100 p-6 dark:bg-zinc-950">
      <div className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-8 text-center shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
        <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-gradient-to-br from-brand-500 to-brand-700 text-sm font-bold tracking-tight text-white shadow-sm dark:from-brand-400 dark:to-brand-600">
          TW
        </div>
        <h1 className="mb-2 text-lg font-semibold text-slate-800 dark:text-zinc-100">
          请在桌面窗口中使用本应用
        </h1>
        <p className="text-sm leading-6 text-slate-500 dark:text-zinc-400">
          当前页面是在浏览器中打开的 Vite 开发服务器，而本应用是 Tauri
          桌面程序，账户、签到、积分等功能均依赖 Rust 后端，无法在纯浏览器环境运行。
        </p>
        <p className="mt-3 text-sm leading-6 text-slate-500 dark:text-zinc-400">
          请关闭此页面，通过桌面窗口使用：
          <br />
          双击项目根目录的 <code className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-xs text-slate-700 dark:bg-zinc-800 dark:text-zinc-300">start.bat</code>
          ，或运行{' '}
          <code className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-xs text-slate-700 dark:bg-zinc-800 dark:text-zinc-300">npm run tauri dev</code>
          {' '}启动。
        </p>
      </div>
    </div>
  );
}

export default function App() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const init = useAppStore((s) => s.init);
  const ready = useAppStore((s) => s.ready);
  const settings = useAppStore((s) => s.settings);
  const [license, setLicense] = useState<'checking' | 'ok' | 'blocked'>('checking');

  // 授权门：release 构建由后端校验本地授权（开发构建后端直接放行）
  useEffect(() => {
    if (!isTauri) return;
    api
      .license.status()
      .then((r) => setLicense(r.status === 'ok' ? 'ok' : 'blocked'))
      .catch(() => setLicense('blocked'));
  }, []);

  useEffect(() => {
    if (license !== 'ok') return;
    void init().catch((err) => {
      console.error('初始化失败:', err);
    });
  }, [license, init]);

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => {
      const theme = useAppStore.getState().settings?.theme ?? 'system';
      const dark = theme === 'dark' || (theme === 'system' && mq.matches);
      document.documentElement.classList.toggle('dark', dark);
      // 同步原生控件(复选框/下拉/滚动条)的渲染主题与应用一致
      document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
    };
    apply();
    mq.addEventListener('change', apply);
    return () => mq.removeEventListener('change', apply);
  }, [settings?.theme]);

  if (!isTauri) {
    return <BrowserFallback />;
  }

  if (license === 'blocked') {
    return <ActivationGate onSuccess={() => setLicense('ok')} />;
  }

  if (license === 'checking' || !ready) {
    return (
      <div className="flex h-full items-center justify-center bg-slate-100 dark:bg-zinc-950">
        <div className="flex flex-col items-center gap-3">
          <div className="h-8 w-8 animate-spin rounded-full border-3 border-zinc-400 border-t-transparent" />
          <span className="text-sm text-slate-500">正在加载…</span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-slate-100 text-slate-800 dark:bg-zinc-950 dark:text-zinc-100">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Sidebar view={view} onNav={setView} />
        <main className="relative flex min-w-0 flex-1 flex-col">
          <TopBar />
          <div
            key={view}
            className="relative min-h-0 flex-1 overflow-auto p-5 animate-[fadeIn_120ms_ease-out]"
          >
            {renderView(view)}
          </div>
        </main>
      </div>
      <Toaster />
    </div>
  );
}
