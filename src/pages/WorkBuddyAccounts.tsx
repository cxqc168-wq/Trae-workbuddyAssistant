// WorkBuddy 账号管理：对齐 Trae Accounts 的表格 UI（筛选 chips + 表格 + 行内操作）
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  CheckCircle2,
  Copy,
  ExternalLink,
  Globe,
  Import,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
  Users,
  Zap,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Modal } from '../components/ui';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import { cn } from '../lib/cn';
import type { WorkBuddyAccountMeta } from '../types';
import { MS_PER_DAY, TokenStatusBadge, displayName, formatTime } from './wb-shared';

type FilterKey = 'all' | 'relogin' | 'checked' | 'unchecked';

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

// 到期时间：临期（<24h）amber 加粗，已过期红
// OAuth 扫码登录：发起 → 打开浏览器 → 轮询采集结果 → 自动入库
function OAuthLoginModal({
  open,
  onClose,
  onDone,
}: {
  open: boolean;
  onClose: () => void;
  onDone: () => Promise<void>;
}) {
  const toast = useAppStore((s) => s.pushToast);
  const [starting, setStarting] = useState(false);
  const [loginId, setLoginId] = useState<string | null>(null);
  const [uri, setUri] = useState('');
  const [error, setError] = useState('');
  const [result, setResult] = useState<WorkBuddyAccountMeta | null>(null);
  // 轮询是否已由本次会话发起（避免重复轮询）
  const pollRef = useRef(0);

  // 打开时重置状态
  useEffect(() => {
    if (open) {
      setStarting(false);
      setLoginId(null);
      setUri('');
      setError('');
      setResult(null);
      pollRef.current += 1;
    }
  }, [open]);

  const start = async () => {
    setStarting(true);
    setError('');
    setResult(null);
    try {
      const res = await api.workbuddy.oauthStart();
      setLoginId(res.loginId);
      setUri(res.verificationUri);
      const { open: openUrl } = await import('@tauri-apps/plugin-shell');
      await openUrl(res.verificationUri);
    } catch (e) {
      setError(`发起登录失败：${String(e)}`);
    } finally {
      setStarting(false);
    }
  };

  // 轮询采集结果：1.5s 一次，直到 done / 出错 / 会话作废
  useEffect(() => {
    if (!open || !loginId) return;
    const sessionId = pollRef.current;
    let cancelled = false;
    let timer: number | undefined;

    const poll = async () => {
      if (pollRef.current !== sessionId) return;
      try {
        const res = await api.workbuddy.oauthPoll(loginId);
        if (pollRef.current !== sessionId) return;
        if (res.done) {
          if (res.result) {
            setResult(res.result);
            toast('success', `已采集账号「${res.result.nickname || res.result.email || res.result.uid || ''}」`);
            await onDone();
          } else {
            setError(res.error || '登录失败');
          }
          return;
        }
      } catch (e) {
        if (!cancelled) setError(`轮询失败：${String(e)}`);
        return;
      }
      if (!cancelled) timer = window.setTimeout(poll, 1500);
    };
    poll();

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [open, loginId, toast, onDone]);

  const copyUri = async () => {
    try {
      await navigator.clipboard.writeText(uri);
      toast('success', '链接已复制');
    } catch {
      toast('error', '复制失败，请手动选择文本复制');
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="扫码登录"
      footer={
        <>
          <button onClick={onClose} className="btn-ghost">
            {result ? '关闭' : '取消'}
          </button>
          {!loginId && !result && (
            <button onClick={() => void start()} disabled={starting} className="btn-primary">
              {starting ? '正在发起…' : '开始扫码登录'}
            </button>
          )}
          {loginId && !result && uri && (
            <button
              onClick={async () => {
                const { open: openUrl } = await import('@tauri-apps/plugin-shell');
                await openUrl(uri);
              }}
              className="btn-outline"
            >
              <ExternalLink size={14} /> 重新打开
            </button>
          )}
          {result && (
            <button onClick={() => void start()} className="btn-primary">
              再登录一个账号
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
        {!loginId && !result && (
          <div className="text-sm text-slate-600 dark:text-zinc-300">
            点击「开始扫码登录」在浏览器中打开 WorkBuddy 官网验证页，微信扫码授权后将自动采集账号并保存，
            无需手动粘贴 token。
          </div>
        )}

        {loginId && !result && (
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-xs text-slate-500 dark:text-zinc-400">
              <Loader2 size={13} className="animate-spin" />
              正在等待浏览器完成授权，请扫码后稍候…
            </div>
            <div>
              <label className="label">验证链接（若浏览器未自动打开可复制到浏览器访问）</label>
              <div className="flex items-start gap-2">
                <textarea
                  readOnly
                  value={uri}
                  className="input min-h-[64px] flex-1 font-mono text-xs"
                  onClick={(e) => (e.target as HTMLTextAreaElement).select()}
                />
                <button onClick={() => void copyUri()} className="btn-ghost !p-2" title="复制链接">
                  <Copy size={14} />
                </button>
              </div>
            </div>
            {error && (
              <div className="rounded-lg border border-rose-300 bg-rose-50 p-3 text-xs text-rose-600 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-300">
                {error}
              </div>
            )}
          </div>
        )}

        {result && (
          <div className="space-y-3">
            <div className="flex items-center gap-2 rounded-lg border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-300">
              <CheckCircle2 size={15} /> 登录成功，账号已自动保存
            </div>
            <div className="grid grid-cols-2 gap-2 text-xs">
              <span className="text-slate-500">昵称</span>
              <span>{result.nickname || '-'}</span>
              <span className="text-slate-500">邮箱</span>
              <span>{result.email || '-'}</span>
              <span className="text-slate-500">uid</span>
              <span className="font-mono">{result.uid || '-'}</span>
              <span className="text-slate-500">企业</span>
              <span>{result.enterpriseName || '-'}</span>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}

function ExpireCell({ account }: { account: WorkBuddyAccountMeta }) {
  if (account.expiresAt == null) {
    return <span className="text-xs text-slate-300">-</span>;
  }
  const remain = account.expiresAt - Date.now();
  return (
    <span
      className={cn(
        'text-xs tabular-nums',
        remain <= 0
          ? 'font-semibold text-rose-500'
          : remain < MS_PER_DAY
            ? 'font-semibold text-amber-500'
            : 'text-slate-500',
      )}
    >
      {formatTime(account.expiresAt)}
    </span>
  );
}

export default function WorkBuddyAccounts() {
  const toast = useAppStore((s) => s.pushToast);

  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[]>([]);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [oauthOpen, setOauthOpen] = useState(false);
  const [filter, setFilter] = useState<FilterKey>('all');
  const [refreshingId, setRefreshingId] = useState<string | null>(null);

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

  const counts = useMemo(
    () => ({
      all: accounts.length,
      relogin: accounts.filter((a) => a.needsRelogin).length,
      checked: accounts.filter((a) => a.checkedToday).length,
      unchecked: accounts.filter((a) => !a.checkedToday).length,
    }),
    [accounts],
  );

  const filtered = useMemo(() => {
    switch (filter) {
      case 'relogin':
        return accounts.filter((a) => a.needsRelogin);
      case 'checked':
        return accounts.filter((a) => a.checkedToday);
      case 'unchecked':
        return accounts.filter((a) => !a.checkedToday);
      default:
        return accounts;
    }
  }, [accounts, filter]);

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

  const onRefreshToken = async (a: WorkBuddyAccountMeta) => {
    setRefreshingId(a.id);
    try {
      await api.workbuddy.refreshToken(a.id);
      toast('success', `账号「${displayName(a)}」token 已刷新`);
      await reload();
    } catch (e) {
      toast('error', `刷新 token 失败：${String(e)}`);
    } finally {
      setRefreshingId(null);
    }
  };

  const chips: { key: FilterKey; label: string; count: number }[] = [
    { key: 'all', label: '全部', count: counts.all },
    { key: 'relogin', label: '需重登', count: counts.relogin },
    { key: 'checked', label: '今日已签', count: counts.checked },
    { key: 'unchecked', label: '今日未签', count: counts.unchecked },
  ];

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="账号管理"
        desc="导入本机或手动添加 WorkBuddy 账号，维护 token 与登录状态"
        actions={
          <>
            <button onClick={() => void reload()} className="btn-outline">
              <RefreshCw size={15} className={loading ? 'animate-spin' : undefined} /> 刷新
            </button>
            <button onClick={() => void importLocal()} disabled={importing} className="btn-outline">
              {importing ? <Loader2 size={15} className="animate-spin" /> : <Import size={15} />}
              导入本机账号
            </button>
            <button onClick={() => setOauthOpen(true)} className="btn-outline">
              <Globe size={15} /> 扫码登录
            </button>
            <button onClick={() => setManualOpen(true)} className="btn-primary">
              <Plus size={15} /> 手动添加
            </button>
          </>
        }
      />

      <div className="mb-3 flex flex-wrap items-center gap-2 text-sm">
        {chips.map((c) => (
          <button
            key={c.key}
            onClick={() => setFilter(c.key)}
            className={`chip border ${
              filter === c.key
                ? 'border-brand-500 text-brand-600'
                : 'border-slate-300 text-slate-500 dark:border-zinc-700 dark:text-zinc-400'
            }`}
          >
            {c.label} ({c.count})
          </button>
        ))}
        {refreshingId && (
          <span className="ml-auto flex items-center gap-1 text-xs text-slate-400">
            <Loader2 size={12} className="animate-spin" /> 正在刷新 token…
          </span>
        )}
      </div>

      <div className="card overflow-hidden">
        {filtered.length === 0 ? (
          <div className="p-6">
            <EmptyState
              icon={<Users size={28} />}
              title={
                accounts.length === 0
                  ? loading
                    ? '正在加载账号…'
                    : '还没有 WorkBuddy 账号'
                  : '此条件下没有账号'
              }
              hint="导入本机已登录的 WorkBuddy 客户端账号，或手动粘贴 access_token 添加。"
            />
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
              <tr>
                <th className="px-4 py-2 text-left">账号</th>
                <th className="px-4 py-2 text-left">企业</th>
                <th className="px-4 py-2 text-left">Token</th>
                <th className="px-4 py-2 text-left">到期时间</th>
                <th className="px-4 py-2 text-left">今日</th>
                <th className="px-4 py-2 text-right">操作</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((a) => {
                return (
                  <tr key={a.id} className="border-t border-slate-200 dark:border-zinc-800">
                    <td className="px-4 py-3">
                      <div className="font-medium">{displayName(a)}</div>
                      <div className="text-xs text-slate-400">{a.email || a.uid || '-'}</div>
                    </td>
                    <td className="max-w-[180px] truncate px-4 py-3 text-xs text-slate-500">
                      {a.enterpriseName || '-'}
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-1">
                        <TokenStatusBadge account={a} />
                        {a.hasRefreshToken && (
                          <span title="支持自动续期" className="text-sky-500">
                            <Zap size={12} />
                          </span>
                        )}
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <ExpireCell account={a} />
                    </td>
                    <td className="px-4 py-3">
                      {a.checkedToday ? (
                        <Badge tone="green">已签</Badge>
                      ) : (
                        <Badge tone="slate">未签</Badge>
                      )}
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1">
                        <button
                          title={a.hasRefreshToken ? '刷新 token' : '刷新 token（无 refresh_token，可能失败）'}
                          onClick={() => void onRefreshToken(a)}
                          disabled={refreshingId != null}
                          className={cn(
                            'btn-ghost !p-2 text-sky-500 hover:bg-sky-50 dark:hover:bg-sky-500/10',
                            refreshingId != null && 'opacity-40 cursor-not-allowed',
                          )}
                        >
                          {refreshingId === a.id ? (
                            <Loader2 size={14} className="animate-spin" />
                          ) : (
                            <Zap size={14} />
                          )}
                        </button>
                        <button
                          title="删除"
                          onClick={() => void onDelete(a)}
                          className="btn-ghost !p-2 text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-500/10"
                        >
                          <Trash2 size={14} />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      <ManualAddModal open={manualOpen} onClose={() => setManualOpen(false)} onAdded={reload} />
      <OAuthLoginModal open={oauthOpen} onClose={() => setOauthOpen(false)} onDone={reload} />
    </div>
  );
}
