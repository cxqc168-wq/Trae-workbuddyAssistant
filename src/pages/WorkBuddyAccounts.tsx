import { useCallback, useEffect, useState } from 'react';
import { CheckCircle2, Import, Loader2, Plus, Trash2, Users } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Modal } from '../components/ui';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import type { WorkBuddyAccountMeta } from '../types';
import { TokenStatusBadge, displayName, formatTime } from './wb-shared';

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

export default function WorkBuddyAccounts() {
  const toast = useAppStore((s) => s.pushToast);

  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[]>([]);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);

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

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="WorkBuddy 账号"
        desc="导入本机或手动添加账号"
        actions={
          <>
            <button onClick={() => void importLocal()} disabled={importing} className="btn-primary">
              {importing ? <Loader2 size={15} className="animate-spin" /> : <Import size={15} />}
              导入本机账号
            </button>
            <button onClick={() => setManualOpen(true)} className="btn-outline">
              <Plus size={15} /> 手动添加
            </button>
            {loading && <Loader2 size={14} className="animate-spin text-slate-400" />}
          </>
        }
      />

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

      <ManualAddModal open={manualOpen} onClose={() => setManualOpen(false)} onAdded={reload} />
    </div>
  );
}
