import { useCallback, useEffect, useState } from 'react';
import { CheckCircle2, Loader2, PlayCircle, Users } from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Progress } from '../components/ui';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import { cn } from '../lib/cn';
import type {
  WorkBuddyAccountMeta,
  WorkBuddyCheckinDone,
  WorkBuddyCheckinProgress,
} from '../types';
import { TokenStatusBadge, displayName } from './wb-shared';

export default function WorkBuddyCheckin() {
  const toast = useAppStore((s) => s.pushToast);
  const checkinRunning = useAppStore((s) => s.wbCheckin.running);
  const checkinResults = useAppStore((s) => s.wbCheckin.results);
  const setWbCheckin = useAppStore((s) => s.setWbCheckin);

  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[]>([]);

  // null 表示「全选」（默认状态）；一旦手动勾选则固定为具体集合
  const [selected, setSelected] = useState<Set<string> | null>(null);
  const [skipCheckedToday, setSkipCheckedToday] = useState(true);

  // 进度总数：来自事件/发起签到；挂载时从 store 既有结果恢复（切页返回接续显示）
  const [checkinTotal, setCheckinTotal] = useState(() => {
    const rs = useAppStore.getState().wbCheckin.results;
    return rs.length > 0 ? rs[rs.length - 1].total : 0;
  });

  const reload = useCallback(async () => {
    try {
      setAccounts(await api.workbuddy.listAccounts());
    } catch (e) {
      toast('error', `读取 WorkBuddy 账号失败：${String(e)}`);
    }
  }, [toast]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // 签到进度事件：后端边执行边推送，完成后推送汇总。
  // 页面卸载时清理监听，但 store 状态保留——回切时重新 listen 接续。
  useEffect(() => {
    const un1 = listen<WorkBuddyCheckinProgress>('workbuddy-checkin-progress', (e) => {
      setCheckinTotal(e.payload.total);
      // 防止 stale closure 丢事件：从 store 取最新结果再追加
      const cur = useAppStore.getState().wbCheckin.results;
      setWbCheckin({ results: [...cur, e.payload] });
    });
    const un2 = listen<WorkBuddyCheckinDone>('workbuddy-checkin-done', (e) => {
      setWbCheckin({ running: false });
      // toast 由发起方（本页或顶栏）在 checkinAll 返回后统一提示，避免双发
      void e.payload;
      void reload();
    });
    return () => {
      void un1.then((f) => f());
      void un2.then((f) => f());
    };
  }, [reload, toast, setWbCheckin]);

  const isSelected = useCallback(
    (id: string) => (selected === null ? true : selected.has(id)),
    [selected],
  );

  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      const base = prev ?? new Set(accounts.map((a) => a.id));
      const next = new Set(base);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // 勾选且未被「跳过今日已签」排除的账号
  const targetIds = accounts
    .filter((a) => isSelected(a.id) && !(skipCheckedToday && a.checkedToday))
    .map((a) => a.id);

  const startCheckinAll = async () => {
    if (targetIds.length === 0) return;
    // 清空上次结果，重置进度
    setWbCheckin({ running: true, results: [] });
    setCheckinTotal(targetIds.length);
    try {
      // 命令阻塞至全部完成并返回全量结果；期间进度通过事件推送
      const entries = await api.workbuddy.checkinAll(targetIds);
      // 覆盖式兜底（事件丢失时结果仍完整）；收尾复位不依赖事件
      setWbCheckin({
        results: entries.map((e, i) => ({ ...e, index: i, total: entries.length })),
        running: false,
      });
      // 发起方负责汇总提示（顶栏发起时由顶栏提示，此处不重复）
      const ok = entries.filter((e) => e.result === 'success').length;
      const already = entries.filter((e) => e.result === 'already').length;
      const failed = entries.length - ok - already;
      toast(
        failed > 0 ? 'warn' : 'success',
        `签到完成：成功 ${ok}，已签 ${already}，失败 ${failed}`,
      );
    } catch (e) {
      setWbCheckin({ running: false });
      toast('error', `签到失败：${String(e)}`);
    }
  };

  return (
    <div className="animate-fade-in">
      <PageHeader title="WorkBuddy 签到" />

      <div className="card mb-4 p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={skipCheckedToday}
              onChange={(e) => setSkipCheckedToday(e.target.checked)}
            />
            跳过今日已签
          </label>
          <div className="flex items-center gap-3">
            <span className="text-xs text-slate-500">
              目标 <b className="tabular-nums">{targetIds.length}</b> 个账号
            </span>
            <button
              onClick={() => void startCheckinAll()}
              disabled={checkinRunning || targetIds.length === 0}
              className="btn-primary"
            >
              {checkinRunning ? (
                <Loader2 size={16} className="animate-spin" />
              ) : (
                <PlayCircle size={16} />
              )}
              {checkinRunning ? '签到进行中…' : '开始签到'}
            </button>
          </div>
        </div>
      </div>

      {accounts.length === 0 ? (
        <EmptyState
          icon={<Users size={28} />}
          title="暂无 WorkBuddy 账号"
          hint="请先切换到「账号管理」页面，导入本机账号或手动添加后再发起签到。"
        />
      ) : (
        <div className="card divide-y divide-slate-100 dark:divide-zinc-800">
          {accounts.map((a) => {
            const skipped = skipCheckedToday && !!a.checkedToday;
            return (
              <label
                key={a.id}
                className={cn('flex items-center gap-3 px-4 py-2.5 text-sm', skipped && 'opacity-60')}
              >
                <input
                  type="checkbox"
                  checked={skipped || isSelected(a.id)}
                  disabled={skipped}
                  onChange={() => toggleSelect(a.id)}
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium text-slate-800 dark:text-zinc-100">
                    {displayName(a)}
                  </div>
                  <div className="truncate text-xs text-slate-400">{a.email || a.uid || '-'}</div>
                </div>
                <TokenStatusBadge account={a} />
                {a.checkedToday && (
                  <Badge tone="blue">
                    <CheckCircle2 size={12} /> 今日已签
                  </Badge>
                )}
              </label>
            );
          })}
        </div>
      )}

      {(checkinRunning || checkinResults.length > 0) && (
        <div className="card mt-4 p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="font-medium text-slate-800 dark:text-zinc-100">签到进度</h3>
            {checkinRunning ? (
              <Badge tone="blue">
                <Loader2 size={12} className="animate-spin" /> 运行中
              </Badge>
            ) : (
              <Badge tone="green">
                <CheckCircle2 size={12} /> 已完成
              </Badge>
            )}
          </div>
          <Progress value={checkinResults.length} max={checkinTotal || 1} />
          <div className="mt-1 text-xs text-slate-500">
            {checkinResults.length}/{checkinTotal || 0}
          </div>
          <div className="mt-3 space-y-1">
            {checkinResults.map((r, i) => (
              <div
                key={`${r.accountId ?? 'na'}-${i}`}
                className="flex items-center gap-2 rounded border border-slate-200 px-3 py-2 text-sm dark:border-zinc-700"
              >
                <span className="w-6 text-right text-xs text-slate-400">{i + 1}</span>
                <span className="min-w-0 flex-1 truncate">{r.email}</span>
                {r.result === 'success' && <Badge tone="green">成功</Badge>}
                {r.result === 'already' && <Badge tone="blue">已签</Badge>}
                {r.result === 'error' && <Badge tone="red">失败</Badge>}
                {!r.result && <Badge tone="slate">进行中</Badge>}
                {r.error && (
                  <span className="max-w-[40%] truncate text-xs text-rose-500" title={r.error}>
                    {r.error}
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
