import { useCallback, useEffect, useState } from 'react';
import { AlertTriangle, Coins, RefreshCw, XCircle } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Progress } from '../components/ui';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import type { WorkBuddyAccountMeta, WorkBuddyCreditSummary } from '../types';
import { formatNum, formatTime } from './wb-shared';

function CreditAccountCard({ summary }: { summary: WorkBuddyCreditSummary }) {
  const total = summary.totalCapacity ?? 0;
  const remaining = summary.totalRemaining ?? 0;
  const failed = !summary.ok || !!summary.error;
  return (
    <div className="card p-4">
      <div className="flex items-center justify-between gap-2">
        <h3 className="truncate font-medium text-slate-800 dark:text-zinc-100">
          {summary.accountName}
        </h3>
        {summary.updatedAt != null && (
          <span className="shrink-0 text-xs text-slate-400">更新于 {formatTime(summary.updatedAt)}</span>
        )}
      </div>
      {failed ? (
        <div className="mt-2 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-600 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-300">
          <XCircle size={12} className="mr-1 inline" />
          {summary.error || '积分查询失败'}
        </div>
      ) : (
        <>
          <div className="mt-3 flex items-baseline gap-2">
            <span className="text-2xl font-semibold tabular-nums text-slate-800 dark:text-zinc-100">
              {formatNum(remaining)}
            </span>
            <span className="text-sm text-slate-400">/ {formatNum(total)}</span>
            {total > 0 && (
              <span className="ml-auto text-xs text-slate-400">
                剩余 {Math.round((remaining / total) * 100)}%
              </span>
            )}
          </div>
          <Progress value={remaining} max={total} className="mt-2" />
          {summary.expiringSoon && (
            <div className="mt-2 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:border-amber-700 dark:bg-amber-900/20 dark:text-amber-300">
              <AlertTriangle size={12} className="mr-1 inline" />
              即将过期额度 {formatNum(summary.expiringSoonRemaining)}
            </div>
          )}
          {summary.resources && summary.resources.length > 0 && (
            <div className="mt-3 space-y-1.5">
              {summary.resources.map((r, i) => (
                <div
                  key={r.packageCode ?? i}
                  className="flex items-center gap-2 rounded-lg border border-slate-200 px-3 py-2 text-xs dark:border-zinc-700"
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium text-slate-700 dark:text-zinc-200">
                      {r.packageName || r.packageCode || '未命名资源'}
                    </div>
                    <div className="mt-0.5 text-slate-400">
                      剩余 {formatNum(r.remaining)} / {formatNum(r.total)}
                      {r.expireAt != null && ` · 到期 ${formatTime(r.expireAt)}`}
                    </div>
                  </div>
                  {r.expired && <Badge tone="red">已过期</Badge>}
                  {!r.expired && r.expiringSoon && <Badge tone="amber">即将过期</Badge>}
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default function WorkBuddyCredits() {
  const toast = useAppStore((s) => s.pushToast);

  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[]>([]);
  const [creditsData, setCreditsData] = useState<WorkBuddyCreditSummary[] | null>(null);
  const [creditsLoading, setCreditsLoading] = useState(false);

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

  const refreshCredits = async () => {
    if (accounts.length === 0) {
      toast('warn', '请先在「账号列表」添加 WorkBuddy 账号');
      return;
    }
    setCreditsLoading(true);
    try {
      const data = await api.workbuddy.credits();
      setCreditsData(data);
      const okCount = data.filter((d) => d.ok).length;
      toast(okCount > 0 ? 'success' : 'warn', `已查询 ${okCount}/${data.length} 个账号的积分`);
    } catch (e) {
      toast('error', `刷新积分失败：${String(e)}`);
    } finally {
      setCreditsLoading(false);
    }
  };

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="WorkBuddy 积分"
        desc="查询全部账号的套餐额度、剩余与到期情况"
        actions={
          <button
            onClick={() => void refreshCredits()}
            disabled={creditsLoading || accounts.length === 0}
            className="btn-primary"
          >
            <RefreshCw size={15} className={creditsLoading ? 'animate-spin' : undefined} />
            刷新积分
          </button>
        }
      />

      {!creditsData ? (
        <EmptyState
          icon={<Coins size={28} />}
          title={accounts.length === 0 ? '请先添加 WorkBuddy 账号' : '尚未加载积分数据'}
          hint="点击「刷新积分」查询全部账号的额度、剩余与到期信息。"
        />
      ) : creditsData.length === 0 ? (
        <EmptyState
          icon={<Coins size={28} />}
          title="暂无积分数据"
          hint="请先在「账号列表」添加账号。"
        />
      ) : (
        <div className="space-y-3">
          {creditsData.map((c, i) => (
            <CreditAccountCard key={c.accountId ?? i} summary={c} />
          ))}
        </div>
      )}
    </div>
  );
}
