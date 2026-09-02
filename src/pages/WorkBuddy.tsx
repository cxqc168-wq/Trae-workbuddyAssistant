import { useCallback, useEffect, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  Coins,
  HelpCircle,
  Import,
  Loader2,
  PlayCircle,
  Plus,
  RefreshCw,
  Trash2,
  Users,
  XCircle,
} from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Modal, Progress } from '../components/ui';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import { cn } from '../lib/cn';
import type {
  WorkBuddyAccountMeta,
  WorkBuddyCheckinDone,
  WorkBuddyCheckinEntry,
  WorkBuddyCheckinProgress,
  WorkBuddyCreditSummary,
  WorkBuddyViewKey,
} from '../types';

const PANELS: { key: WorkBuddyViewKey; label: string; icon: typeof Users }[] = [
  { key: 'accounts', label: '账号列表', icon: Users },
  { key: 'checkin', label: '一键签到', icon: PlayCircle },
  { key: 'credits', label: '积分概览', icon: Coins },
];

const MS_PER_DAY = 24 * 3600 * 1000;

function displayName(a: WorkBuddyAccountMeta): string {
  return a.nickname || a.email || a.uid || '未命名账号';
}

function formatNum(n: number | null | undefined): string {
  if (n == null || Number.isNaN(n)) return '-';
  return n.toLocaleString('zh-CN', { maximumFractionDigits: 2 });
}

function formatTime(ms: number | null | undefined): string {
  if (ms == null) return '-';
  return new Date(ms).toLocaleString('zh-CN');
}

// token 状态：需重登 > 已过期/临期(<24h)/有效 > 未知
function TokenStatusBadge({ account }: { account: WorkBuddyAccountMeta }) {
  if (account.needsRelogin) {
    return (
      <span title={account.needsReloginReason ?? '需要重新登录'}>
        <Badge tone="red">
          <AlertTriangle size={12} /> 需重登
        </Badge>
      </span>
    );
  }
  if (account.expiresAt == null) {
    return (
      <Badge tone="slate">
        <HelpCircle size={12} /> 未知
      </Badge>
    );
  }
  const remain = account.expiresAt - Date.now();
  if (remain <= 0) {
    return (
      <Badge tone="red">
        <XCircle size={12} /> 已过期
      </Badge>
    );
  }
  if (remain < MS_PER_DAY) {
    return (
      <Badge tone="amber">
        <AlertTriangle size={12} /> 临期
      </Badge>
    );
  }
  return (
    <Badge tone="green">
      <CheckCircle2 size={12} /> 有效
    </Badge>
  );
}

function AccountCard({
  account,
  onDelete,
}: {
  account: WorkBuddyAccountMeta;
  onDelete: (a: WorkBuddyAccountMeta) => void;
}) {
  const sub = [account.email, account.uid].filter(Boolean).join(' · ');
  return (
    <div className="card flex items-start justify-between gap-3 p-4">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate font-medium text-slate-800 dark:text-zinc-100">
            {displayName(account)}
          </span>
          {account.checkedToday && (
            <Badge tone="blue">
              <CheckCircle2 size={12} /> 今日已签
            </Badge>
          )}
        </div>
        {sub && <div className="mt-0.5 truncate text-xs text-slate-400">{sub}</div>}
        {account.enterpriseName && (
          <div className="mt-0.5 truncate text-xs text-slate-500 dark:text-zinc-400">
            {account.enterpriseName}
          </div>
        )}
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <TokenStatusBadge account={account} />
          {account.hasRefreshToken && <Badge tone="slate">可刷新</Badge>}
          {account.expiresAt != null && (
            <span className="text-xs text-slate-400">到期 {formatTime(account.expiresAt)}</span>
          )}
        </div>
      </div>
      <button
        title="删除账号"
        onClick={() => onDelete(account)}
        className="btn-ghost !p-2 text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-500/10"
      >
        <Trash2 size={14} />
      </button>
    </div>
  );
}

function ManualAddModal({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: () => Promise<void>;
}) {
  const toast = useAppStore((s) => s.pushToast);
  const [accessToken, setAccessToken] = useState('');
  const [refreshToken, setRefreshToken] = useState('');
  const [uid, setUid] = useState('');
  const [nickname, setNickname] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setAccessToken('');
      setRefreshToken('');
      setUid('');
      setNickname('');
      setBusy(false);
    }
  }, [open]);

  const submit = async () => {
    if (!accessToken.trim()) return;
    setBusy(true);
    try {
      await api.workbuddy.addManual({
        access_token: accessToken.trim(),
        refresh_token: refreshToken.trim() || undefined,
        uid: uid.trim() || undefined,
        nickname: nickname.trim() || undefined,
      });
      toast('success', 'WorkBuddy 账号已添加');
      onClose();
      await onAdded();
    } catch (e) {
      toast('error', `添加失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="手动添加 WorkBuddy 账号"
      footer={
        <>
          <button onClick={onClose} className="btn-ghost">
            取消
          </button>
          <button onClick={submit} disabled={busy || !accessToken.trim()} className="btn-primary">
            {busy ? '添加中…' : '添加'}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <div>
          <label className="label">access_token（必填）</label>
          <textarea
            value={accessToken}
            onChange={(e) => setAccessToken(e.target.value)}
            className="input min-h-[90px] font-mono text-xs"
            placeholder="粘贴 WorkBuddy 的 access_token"
          />
        </div>
        <div>
          <label className="label">refresh_token（选填，用于自动续期）</label>
          <textarea
            value={refreshToken}
            onChange={(e) => setRefreshToken(e.target.value)}
            className="input min-h-[70px] font-mono text-xs"
            placeholder="粘贴 refresh_token（可选）"
          />
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="label">uid（选填）</label>
            <input
              value={uid}
              onChange={(e) => setUid(e.target.value)}
              className="input"
              placeholder="账号 uid"
            />
          </div>
          <div>
            <label className="label">昵称（选填）</label>
            <input
              value={nickname}
              onChange={(e) => setNickname(e.target.value)}
              className="input"
              placeholder="账号昵称"
            />
          </div>
        </div>
      </div>
    </Modal>
  );
}

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

export default function WorkBuddy() {
  const toast = useAppStore((s) => s.pushToast);

  const [panel, setPanel] = useState<WorkBuddyViewKey>('accounts');
  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[]>([]);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);

  // null 表示「全选」（默认状态）；一旦手动勾选则固定为具体集合
  const [selected, setSelected] = useState<Set<string> | null>(null);
  const [skipCheckedToday, setSkipCheckedToday] = useState(true);

  const [checkinRunning, setCheckinRunning] = useState(false);
  const [checkinResults, setCheckinResults] = useState<WorkBuddyCheckinEntry[]>([]);
  const [checkinTotal, setCheckinTotal] = useState(0);

  const [creditsData, setCreditsData] = useState<WorkBuddyCreditSummary[] | null>(null);
  const [creditsLoading, setCreditsLoading] = useState(false);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setAccounts(await api.workbuddy.listAccounts());
    } catch (e) {
      toast('error', `读取 WorkBuddy 账号失败：${String(e)}`);
    } finally {
      setLoading(false);
    }
  }, [toast]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // 签到进度事件：后端边执行边推送，完成后推送汇总
  useEffect(() => {
    const un1 = listen<WorkBuddyCheckinProgress>('workbuddy-checkin-progress', (e) => {
      setCheckinTotal(e.payload.total);
      setCheckinResults((prev) => [...prev, e.payload]);
    });
    const un2 = listen<WorkBuddyCheckinDone>('workbuddy-checkin-done', (e) => {
      setCheckinRunning(false);
      const d = e.payload;
      toast(
        d.failed > 0 ? 'warn' : 'success',
        `签到完成：成功 ${d.ok}，已签 ${d.already}，失败 ${d.failed}`,
      );
      void reload();
    });
    return () => {
      void un1.then((f) => f());
      void un2.then((f) => f());
    };
  }, [reload, toast]);

  const importLocal = async () => {
    setImporting(true);
    try {
      const acc = await api.workbuddy.importLocal();
      toast('success', `已导入账号「${displayName(acc)}」`);
      await reload();
    } catch (e) {
      toast('error', `导入本机账号失败：${String(e)}`);
    } finally {
      setImporting(false);
    }
  };

  const onDelete = async (a: WorkBuddyAccountMeta) => {
    if (!window.confirm(`确认删除 WorkBuddy 账号「${displayName(a)}」？`)) return;
    try {
      await api.workbuddy.deleteAccount(a.id);
      toast('info', '账号已删除');
      await reload();
    } catch (e) {
      toast('error', `删除失败：${String(e)}`);
    }
  };

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
    setCheckinResults([]);
    setCheckinTotal(targetIds.length);
    setCheckinRunning(true);
    try {
      // 命令阻塞至全部完成并返回全量结果；期间进度通过事件推送
      const entries = await api.workbuddy.checkinAll(targetIds);
      // 覆盖式兜底（事件丢失时结果仍完整）；收尾复位不依赖事件；toast+reload 由 done 事件负责
      setCheckinResults(entries);
      setCheckinRunning(false);
    } catch (e) {
      setCheckinRunning(false);
      toast('error', `签到失败：${String(e)}`);
    }
  };

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

  const panelIndex = PANELS.findIndex((p) => p.key === panel);

  return (
    <div className="flex h-full animate-fade-in flex-col">
      <PageHeader title="WorkBuddy" desc="WorkBuddy 账号管理、批量签到与积分概览" />

      {/* 顶部按钮组：三按钮等宽切换面板 */}
      <div className="mb-4 flex gap-2">
        {PANELS.map((p) => {
          const Icon = p.icon;
          const active = panel === p.key;
          return (
            <button
              key={p.key}
              onClick={() => setPanel(p.key)}
              className={cn(
                'flex flex-1 items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition active:scale-[0.98]',
                active
                  ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
                  : 'bg-white text-slate-600 hover:bg-slate-100 dark:bg-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800',
              )}
            >
              <Icon size={16} />
              {p.label}
            </button>
          );
        })}
      </div>

      {/* 左右滑动容器：三面板横排，translateX 切换，保持挂载不卸载以保留状态 */}
      <div className="min-h-0 flex-1 overflow-hidden">
        <div
          className="flex h-full w-full transition-transform duration-300 ease-out"
          style={{ transform: `translateX(-${panelIndex * 100}%)` }}
        >
          {/* 面板一：账号列表 */}
          <div className="h-full w-full shrink-0 overflow-y-auto pr-1">
            <div className="mb-4 flex flex-wrap items-center gap-2">
              <button onClick={() => void importLocal()} disabled={importing} className="btn-primary">
                {importing ? <Loader2 size={15} className="animate-spin" /> : <Import size={15} />}
                导入本机账号
              </button>
              <button onClick={() => setManualOpen(true)} className="btn-outline">
                <Plus size={15} /> 手动添加
              </button>
              {loading && <Loader2 size={14} className="animate-spin text-slate-400" />}
            </div>

            {accounts.length === 0 ? (
              <div className="flex flex-col items-center gap-4">
                <EmptyState
                  icon={<Users size={28} />}
                  title={loading ? '正在加载账号…' : '还没有 WorkBuddy 账号'}
                  hint="导入本机已登录的 WorkBuddy 客户端账号，或手动粘贴 access_token 添加。"
                />
                {!loading && (
                  <div className="flex gap-2">
                    <button onClick={() => void importLocal()} disabled={importing} className="btn-primary">
                      {importing ? (
                        <Loader2 size={15} className="animate-spin" />
                      ) : (
                        <Import size={15} />
                      )}
                      导入本机账号
                    </button>
                    <button onClick={() => setManualOpen(true)} className="btn-outline">
                      <Plus size={15} /> 手动添加
                    </button>
                  </div>
                )}
              </div>
            ) : (
              <div className="space-y-3">
                {accounts.map((a) => (
                  <AccountCard key={a.id} account={a} onDelete={(x) => void onDelete(x)} />
                ))}
              </div>
            )}
          </div>

          {/* 面板二：一键签到 */}
          <div className="h-full w-full shrink-0 overflow-y-auto pr-1">
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
                hint="请先切换到「账号列表」面板，导入本机账号或手动添加后再发起签到。"
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

          {/* 面板三：积分概览 */}
          <div className="h-full w-full shrink-0 overflow-y-auto pr-1">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
              <span className="text-sm text-slate-500 dark:text-zinc-400">
                查询全部账号的套餐额度、剩余与到期情况
              </span>
              <button
                onClick={() => void refreshCredits()}
                disabled={creditsLoading || accounts.length === 0}
                className="btn-primary"
              >
                <RefreshCw size={15} className={creditsLoading ? 'animate-spin' : undefined} />
                刷新积分
              </button>
            </div>

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
        </div>
      </div>

      <ManualAddModal open={manualOpen} onClose={() => setManualOpen(false)} onAdded={reload} />
    </div>
  );
}
