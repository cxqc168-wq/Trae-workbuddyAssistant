import {
  LayoutDashboard,
  Users,
  PlayCircle,
  Coins,
  ScrollText,
  Settings,
  Gift,
  Server,
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

const MODULE_LABELS: { key: ModuleKey; label: string }[] = [
  { key: 'trae', label: 'TRAE' },
  { key: 'workbuddy', label: 'WorkBuddy' },
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
          <span
            aria-hidden
            className={cn(
              'absolute inset-y-1 left-1 w-[calc(50%-4px)] rounded-full bg-zinc-900 transition-transform duration-300 ease-out dark:bg-zinc-100',
              module === 'workbuddy' && 'translate-x-full',
            )}
          />
          {MODULE_LABELS.map((m) => (
            <button
              key={m.key}
              onClick={() => setModule(m.key)}
              className={cn(
                'relative z-10 flex flex-1 items-center justify-center rounded-full py-1.5 text-xs font-semibold transition-colors',
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
