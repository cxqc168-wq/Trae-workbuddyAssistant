// WorkBuddy 账号管理：对齐 Trae Accounts 的表格 UI（筛选 chips + 表格 + 行内操作）
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  CheckCircle2,
  Copy,
  Database,
  Download,
  ExternalLink,
  Globe,
  Import,
  Loader2,
  LogIn,
  MessageSquare,
  Plus,
  RefreshCw,
  Repeat,
  Save,
  Trash2,
  Upload,
  Users,
  Zap,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Modal } from '../components/ui';
import { api } from '../lib/tauri';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store';
import { cn } from '../lib/cn';
import type { WorkBuddyAccountMeta, WbStoredAuth, WbSession, WbSessionJob, TraeChatSession } from '../types';
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

// 客户端切换确认弹窗：选择是否重启客户端、是否跟随会话（M2 生效）
function SwitchConfirmModal({
  open,
  account,
  onClose,
  onConfirm,
  busy,
}: {
  open: boolean;
  account: WorkBuddyAccountMeta | null;
  onClose: () => void;
  onConfirm: (reload: boolean, migrate: 'auto' | 'off') => void;
  busy: boolean;
}) {
  const [reload, setReload] = useState(true);
  const [migrate, setMigrate] = useState<'auto' | 'off'>('off');

  useEffect(() => {
    if (open) {
      setReload(true);
      setMigrate('off');
    }
  }, [open]);

  if (!account) return null;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="切换 WorkBuddy 客户端登录"
      footer={
        <>
          <button onClick={onClose} className="btn-ghost" disabled={busy}>
            取消
          </button>
          <button
            onClick={() => onConfirm(reload, migrate)}
            disabled={busy}
            className="btn-primary"
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <LogIn size={14} />}
            {busy ? '切换中…' : '确认切换'}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="rounded-lg border border-slate-200 bg-slate-50 p-3 text-sm dark:border-zinc-700 dark:bg-zinc-800/50">
          <div className="text-xs text-slate-500">将切换到账号</div>
          <div className="mt-1 font-medium">{displayName(account)}</div>
          <div className="text-xs text-slate-400">{account.email || account.uid || '-'}</div>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={reload}
            onChange={(e) => setReload(e.target.checked)}
            className="h-4 w-4"
          />
          切换后自动重启 WorkBuddy 客户端
        </label>

        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={migrate === 'auto'}
            onChange={(e) => setMigrate(e.target.checked ? 'auto' : 'off')}
            className="h-4 w-4"
          />
          切换后自动迁移对话记录到新账号
          <span className="text-xs text-slate-400">（后台执行，可在会话管理中查看进度）</span>
        </label>

        <div className="text-xs text-slate-400">
          切换原理：将该账号的登录文件原子写回 WorkBuddy 官方目录，不修改 token 库。切换前请确保该账号已执行过「备份当前登录」。
        </div>
      </div>
    </Modal>
  );
}

// 会话管理抽屉：浏览当前账号会话，迁移到其他账号
function SessionDrawer({
  open,
  onClose,
  accounts,
  currentUid,
}: {
  open: boolean;
  onClose: () => void;
  accounts: WorkBuddyAccountMeta[];
  currentUid: string | null;
}) {
  const toast = useAppStore((s) => s.pushToast);
  const [sessions, setSessions] = useState<WbSession[]>([]);
  const [loading, setLoading] = useState(false);
  const [targetUid, setTargetUid] = useState('');
  const [migrating, setMigrating] = useState(false);
  const [job, setJob] = useState<WbSessionJob | null>(null);

  const loadSessions = useCallback(async () => {
    if (!currentUid) return;
    setLoading(true);
    try {
      setSessions(await api.workbuddy.sessionList(currentUid));
    } catch (e) {
      toast('error', `读取会话失败：${String(e)}`);
    } finally {
      setLoading(false);
    }
  }, [currentUid, toast]);

  useEffect(() => {
    if (open) {
      void loadSessions();
      // 读取最近 job
      api.workbuddy.sessionJobStatus().then(setJob).catch(() => {});
    }
  }, [open, loadSessions]);

  // 实时监听会话迁移进度事件
  useEffect(() => {
    if (!open) return;
    let unlisten: (() => void) | null = null;
    listen<WbSessionJob & { error?: string }>('wb-session-job', (e) => {
      setJob(e.payload);
      // 迁移完成后刷新会话列表
      if (e.payload.status === 'done' || e.payload.status === 'partial') {
        void loadSessions();
      }
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, [open, loadSessions]);

  const targetAccounts = accounts.filter((a) => a.uid && a.uid !== currentUid);

  const onCopyOne = async (s: WbSession) => {
    if (!targetUid || !currentUid) {
      toast('error', '请先选择目标账号');
      return;
    }
    setMigrating(true);
    try {
      const res = await api.workbuddy.sessionCopy(currentUid, s.id, targetUid);
      toast(res.status === 'copied' ? 'success' : 'info', `会话已迁移 [${res.status}]`);
      await loadSessions();
    } catch (e) {
      toast('error', `迁移失败：${String(e)}`);
    } finally {
      setMigrating(false);
    }
  };

  const onMigrateAll = async () => {
    if (!targetUid || !currentUid) {
      toast('error', '请先选择目标账号');
      return;
    }
    if (!window.confirm(`确认将当前账号的 ${sessions.length} 个会话全部迁移到目标账号？`)) return;
    setMigrating(true);
    try {
      const j = await api.workbuddy.sessionMigrateAll(currentUid, targetUid);
      setJob(j);
      toast('info', `已启动批量迁移（共 ${j.total} 个会话），后台执行中…`);
    } catch (e) {
      toast('error', `启动迁移失败：${String(e)}`);
    } finally {
      setMigrating(false);
    }
  };

  const onExportOne = async (s: WbSession) => {
    const password = window.prompt('请输入加密密码（至少 4 位）：');
    if (!password || password.length < 4) {
      toast('error', '密码至少 4 位');
      return;
    }
    setMigrating(true);
    try {
      const path = await api.workbuddy.sessionExport(s.id, password);
      toast('success', `已导出到：${path}`);
    } catch (e) {
      toast('error', `导出失败：${String(e)}`);
    } finally {
      setMigrating(false);
    }
  };

  const onImportWds = async () => {
    const wdsPath = window.prompt('请输入 .wds 文件的完整路径：');
    if (!wdsPath) return;
    const password = window.prompt('请输入解密密码：');
    if (!password) return;
    setMigrating(true);
    try {
      const newId = await api.workbuddy.sessionImport(wdsPath, password);
      toast('success', `已导入，新会话 ID：${newId}`);
      await loadSessions();
    } catch (e) {
      toast('error', `导入失败：${String(e)}`);
    } finally {
      setMigrating(false);
    }
  };

  const jobProgress = job && job.total > 0 ? Math.round((job.processed / job.total) * 100) : 0;

  return (
    <Modal open={open} onClose={onClose} title="会话管理与迁移" size="xl">
      <div className="space-y-4">
        <div className="flex items-center gap-3">
          <div className="flex-1">
            <label className="label">迁移到目标账号</label>
            <select
              value={targetUid}
              onChange={(e) => setTargetUid(e.target.value)}
              className="input"
            >
              <option value="">请选择目标账号…</option>
              {targetAccounts.map((a) => (
                <option key={a.id} value={a.uid || ''}>
                  {displayName(a)}（{a.uid}）
                </option>
              ))}
            </select>
          </div>
          <button
            onClick={() => void onMigrateAll()}
            disabled={migrating || !targetUid || sessions.length === 0}
            className="btn-primary mt-5"
          >
            {migrating ? <Loader2 size={14} className="animate-spin" /> : <Repeat size={14} />}
            全部迁移（{sessions.length}）
          </button>
          <button
            onClick={() => void onImportWds()}
            disabled={migrating}
            className="btn-outline mt-5"
            title="从 .wds 加密归档文件导入会话"
          >
            <Upload size={14} />
            导入 .wds
          </button>
          <button onClick={() => void loadSessions()} className="btn-outline mt-5" disabled={loading}>
            <RefreshCw size={14} className={loading ? 'animate-spin' : undefined} />
          </button>
        </div>

        {job && job.status !== 'done' && (
          <div className="rounded-lg border border-slate-200 bg-slate-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/50">
            <div className="mb-1 flex items-center justify-between text-xs">
              <span className="text-slate-500">
                迁移进度：{job.processed}/{job.total}（成功 {job.copied}，跳过 {job.skipped}，失败 {job.failed}）
              </span>
              <span className="text-slate-400">{job.status}</span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-slate-200 dark:bg-zinc-700">
              <div
                className="h-full rounded-full bg-brand-500 transition-all"
                style={{ width: `${jobProgress}%` }}
              />
            </div>
          </div>
        )}

        <div className="max-h-[50vh] overflow-y-auto rounded-lg border border-slate-200 dark:border-zinc-700">
          {loading ? (
            <div className="flex items-center justify-center p-8 text-sm text-slate-400">
              <Loader2 size={16} className="mr-2 animate-spin" /> 正在加载会话…
            </div>
          ) : sessions.length === 0 ? (
            <div className="p-8 text-center text-sm text-slate-400">当前账号暂无会话</div>
          ) : (
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
                <tr>
                  <th className="px-3 py-2 text-left">会话标题</th>
                  <th className="px-3 py-2 text-left">工作区</th>
                  <th className="px-3 py-2 text-left">更新时间</th>
                  <th className="px-3 py-2 text-left">文件</th>
                  <th className="px-3 py-2 text-right">操作</th>
                </tr>
              </thead>
              <tbody>
                {sessions.map((s) => (
                  <tr key={s.id} className="border-t border-slate-100 dark:border-zinc-800">
                    <td className="max-w-[200px] truncate px-3 py-2 font-medium">
                      {s.custom_title || s.title || '(无标题)'}
                    </td>
                    <td className="max-w-[180px] truncate px-3 py-2 text-xs text-slate-400">
                      {s.cwd || '-'}
                    </td>
                    <td className="px-3 py-2 text-xs text-slate-400">
                      {s.updated_at ? new Date(s.updated_at).toLocaleString() : '-'}
                    </td>
                    <td className="px-3 py-2 text-xs text-slate-400">{s.file_count || 0}</td>
                    <td className="px-3 py-2 text-right">
                      <button
                        title="导出为 .wds 加密归档"
                        onClick={() => void onExportOne(s)}
                        disabled={migrating}
                        className="btn-ghost !p-1.5 text-sky-600 hover:bg-sky-50 dark:hover:bg-sky-500/10 disabled:opacity-40"
                      >
                        <Download size={13} />
                      </button>
                      <button
                        title="迁移此会话到目标账号"
                        onClick={() => void onCopyOne(s)}
                        disabled={migrating || !targetUid}
                        className="btn-ghost !p-1.5 text-emerald-600 hover:bg-emerald-50 dark:hover:bg-emerald-500/10 disabled:opacity-40"
                      >
                        <Repeat size={13} />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        <div className="text-xs text-slate-400">
          迁移原理：在 workbuddy.db 中复制会话记录（归属改为目标账号）并复制关联文件，原会话保留不变。
        </div>
      </div>
    </Modal>
  );
}

// Trae 对话解密导出：扫描进程内存密钥 → 解密 SQLCipher4 数据库 → 导出对话
function TraeChatModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const toast = useAppStore((s) => s.pushToast);
  const [sessions, setSessions] = useState<TraeChatSession[]>([]);
  const [loading, setLoading] = useState(false);
  const [plainDbPath, setPlainDbPath] = useState('');
  const [exportingId, setExportingId] = useState<string | null>(null);

  const onScan = async () => {
    setLoading(true);
    try {
      const res = await api.trae.sessionList();
      setSessions(res.sessions);
      setPlainDbPath(res.plain_db_path);
      toast('success', `已解密 Trae 数据库，共 ${res.sessions.length} 个会话`);
    } catch (e) {
      toast('error', `解密失败：${String(e)}`);
    } finally {
      setLoading(false);
    }
  };

  const onExport = async (s: TraeChatSession) => {
    setExportingId(s.id);
    try {
      const path = await api.trae.exportMessages(s.id);
      toast('success', `已导出到：${path}`);
    } catch (e) {
      toast('error', `导出失败：${String(e)}`);
    } finally {
      setExportingId(null);
    }
  };

  return (
    <Modal open={open} onClose={onClose} title="Trae 对话解密导出" size="xl">
      <div className="space-y-4">
        <div className="flex items-center gap-3">
          <button onClick={() => void onScan()} disabled={loading} className="btn-primary">
            {loading ? <Loader2 size={14} className="animate-spin" /> : <Database size={14} />}
            扫描并解密
          </button>
          {plainDbPath && (
            <span className="text-xs text-slate-400">明文库：{plainDbPath}</span>
          )}
        </div>

        <div className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-400">
          原理：从运行中的 Trae 进程内存扫描 SQLCipher 密钥，页面级解密 database.db，
          然后读取 chat_session / chat_message 表。使用前请确保 Trae 正在运行。
        </div>

        <div className="max-h-[50vh] overflow-y-auto rounded-lg border border-slate-200 dark:border-zinc-700">
          {loading ? (
            <div className="flex items-center justify-center p-8 text-sm text-slate-400">
              <Loader2 size={16} className="mr-2 animate-spin" /> 正在扫描进程内存并解密…
            </div>
          ) : sessions.length === 0 ? (
            <div className="p-8 text-center text-sm text-slate-400">
              点击「扫描并解密」读取 Trae 对话
            </div>
          ) : (
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
                <tr>
                  <th className="px-3 py-2 text-left">会话标题</th>
                  <th className="px-3 py-2 text-left">消息数</th>
                  <th className="px-3 py-2 text-left">创建时间</th>
                  <th className="px-3 py-2 text-right">操作</th>
                </tr>
              </thead>
              <tbody>
                {sessions.map((s) => (
                  <tr key={s.id} className="border-t border-slate-100 dark:border-zinc-800">
                    <td className="max-w-[300px] truncate px-3 py-2 font-medium">
                      {s.title || '(无标题)'}
                    </td>
                    <td className="px-3 py-2 text-xs text-slate-400">{s.message_count}</td>
                    <td className="px-3 py-2 text-xs text-slate-400">
                      {s.created_at ? new Date(s.created_at).toLocaleString() : '-'}
                    </td>
                    <td className="px-3 py-2 text-right">
                      <button
                        title="导出此会话消息为 JSON"
                        onClick={() => void onExport(s)}
                        disabled={exportingId !== null}
                        className="btn-ghost !p-1.5 text-sky-600 hover:bg-sky-50 dark:hover:bg-sky-500/10 disabled:opacity-40"
                      >
                        {exportingId === s.id ? (
                          <Loader2 size={13} className="animate-spin" />
                        ) : (
                          <Download size={13} />
                        )}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
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
  // M1：客户端登录态切换
  const [storedAuths, setStoredAuths] = useState<WbStoredAuth[]>([]);
  const [backingUp, setBackingUp] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [switchTarget, setSwitchTarget] = useState<WorkBuddyAccountMeta | null>(null);
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false);
  const [traeModalOpen, setTraeModalOpen] = useState(false);
  const [currentWbUid, setCurrentWbUid] = useState<string | null>(null);

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

  const reloadAuths = useCallback(async () => {
    try {
      setStoredAuths(await api.workbuddy.authList());
    } catch {
      // 备份列表读取失败不阻断主界面
    }
    try {
      const cur = await api.workbuddy.authCurrent();
      setCurrentWbUid(cur?.uid ?? null);
    } catch {
      // 当前登录读取失败不阻断
    }
  }, []);

  useEffect(() => {
    void reload();
    void reloadAuths();
  }, [reload, reloadAuths]);

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

  // M1：备份当前 WorkBuddy 客户端登录文件
  const onBackupCurrent = async () => {
    setBackingUp(true);
    try {
      const info = await api.workbuddy.backupCurrentAuth();
      toast('success', `已备份客户端登录：${info.nickname || info.uid}`);
      await reloadAuths();
    } catch (e) {
      toast('error', `备份失败：${String(e)}`);
    } finally {
      setBackingUp(false);
    }
  };

  // M1：切换 WorkBuddy 客户端登录
  const onSwitchConfirm = async (reloadClient: boolean, migrate: 'auto' | 'off') => {
    if (!switchTarget) return;
    const uid = switchTarget.uid || '';
    if (!uid) {
      toast('error', '该账号缺少 uid，无法切换客户端登录');
      return;
    }
    const hasBackup = storedAuths.some((s) => s.uid === uid);
    if (!hasBackup) {
      toast('error', '该账号尚未备份客户端登录，请先在该账号登录状态下点「备份当前登录」');
      return;
    }
    setSwitching(true);
    try {
      const res = await api.workbuddy.switchClient(uid, {
        reload: reloadClient,
        migrate,
      });
      toast(res.reloaded ? 'success' : 'info', res.hint);
      setSwitchTarget(null);
      await reloadAuths();
    } catch (e) {
      toast('error', `切换失败：${String(e)}`);
    } finally {
      setSwitching(false);
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
            <button onClick={() => void onBackupCurrent()} disabled={backingUp} className="btn-outline">
              {backingUp ? <Loader2 size={15} className="animate-spin" /> : <Save size={15} />}
              备份当前登录
            </button>
            <button onClick={() => setSessionDrawerOpen(true)} className="btn-outline">
              <MessageSquare size={15} />
              会话管理
            </button>
            <button onClick={() => setTraeModalOpen(true)} className="btn-outline">
              <Database size={15} />
              Trae 对话
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
          <div className="overflow-x-auto">
            <table className="w-full min-w-[720px] text-sm">
              <thead className="bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
                <tr>
                  <th className="px-4 py-2 text-left whitespace-nowrap">账号</th>
                  <th className="px-4 py-2 text-left whitespace-nowrap">企业</th>
                  <th className="px-4 py-2 text-left whitespace-nowrap">Token</th>
                  <th className="px-4 py-2 text-left whitespace-nowrap">到期时间</th>
                  <th className="px-4 py-2 text-left whitespace-nowrap">今日</th>
                  <th className="px-4 py-2 text-right whitespace-nowrap">操作</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((a) => {
                  return (
                    <tr key={a.id} className="border-t border-slate-200 dark:border-zinc-800">
                      <td className="px-4 py-3 whitespace-nowrap">
                        <div className="font-medium">{displayName(a)}</div>
                        <div className="text-xs text-slate-400">{a.email || a.uid || '-'}</div>
                      </td>
                      <td className="max-w-[180px] truncate px-4 py-3 text-xs text-slate-500">
                        {a.enterpriseName || '-'}
                      </td>
                      <td className="px-4 py-3 whitespace-nowrap">
                        <div className="flex items-center gap-1">
                          <TokenStatusBadge account={a} />
                          {a.hasRefreshToken && (
                            <span title="支持自动续期" className="text-sky-500">
                              <Zap size={12} />
                            </span>
                          )}
                        </div>
                      </td>
                      <td className="px-4 py-3 whitespace-nowrap">
                        <ExpireCell account={a} />
                      </td>
                      <td className="px-4 py-3 whitespace-nowrap">
                        {a.checkedToday ? (
                          <Badge tone="green">已签</Badge>
                        ) : (
                          <Badge tone="slate">未签</Badge>
                        )}
                      </td>
                      <td className="px-4 py-3 whitespace-nowrap">
                        <div className="flex justify-end gap-1">
                          <button
                            title={a.uid && storedAuths.some((s) => s.uid === a.uid)
                              ? '切换 WorkBuddy 客户端到此账号'
                              : '该账号尚未备份客户端登录，请先备份'}
                            onClick={() => setSwitchTarget(a)}
                            disabled={switching}
                            className={cn(
                              'btn-ghost !p-2 text-emerald-600 hover:bg-emerald-50 dark:hover:bg-emerald-500/10',
                              switching && 'opacity-40 cursor-not-allowed',
                            )}
                          >
                            <Repeat size={14} />
                          </button>
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
          </div>
        )}
      </div>

      <ManualAddModal open={manualOpen} onClose={() => setManualOpen(false)} onAdded={reload} />
      <OAuthLoginModal open={oauthOpen} onClose={() => setOauthOpen(false)} onDone={reload} />
      <SwitchConfirmModal
        open={switchTarget !== null}
        account={switchTarget}
        onClose={() => setSwitchTarget(null)}
        onConfirm={onSwitchConfirm}
        busy={switching}
      />
      <SessionDrawer
        open={sessionDrawerOpen}
        onClose={() => setSessionDrawerOpen(false)}
        accounts={accounts}
        currentUid={currentWbUid}
      />
      <TraeChatModal open={traeModalOpen} onClose={() => setTraeModalOpen(false)} />
    </div>
  );
}
