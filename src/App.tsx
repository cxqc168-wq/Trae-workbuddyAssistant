import { useEffect } from 'react';
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
import WorkBuddyAccounts from './pages/WorkBuddyAccounts';
import WorkBuddyCheckin from './pages/WorkBuddyCheckin';
import WorkBuddyCredits from './pages/WorkBuddyCredits';
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

export default function App() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const init = useAppStore((s) => s.init);
  const ready = useAppStore((s) => s.ready);
  const settings = useAppStore((s) => s.settings);

  useEffect(() => {
    void init().catch((err) => {
      console.error('初始化失败:', err);
    });
  }, [init]);

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => {
      const theme = useAppStore.getState().settings?.theme ?? 'system';
      const dark = theme === 'dark' || (theme === 'system' && mq.matches);
      document.documentElement.classList.toggle('dark', dark);
    };
    apply();
    mq.addEventListener('change', apply);
    return () => mq.removeEventListener('change', apply);
  }, [settings?.theme]);

  if (!ready) {
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
          <div className="relative min-h-0 flex-1 overflow-auto p-5">{renderView(view)}</div>
        </main>
      </div>
      <Toaster />
    </div>
  );
}
