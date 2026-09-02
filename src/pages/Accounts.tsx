import { useEffect, useMemo, useState } from 'react';
import {
  Plus,
  Trash2,
  RefreshCw,
  RotateCcw,
  LogIn,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  HelpCircle,
  KeyRound,
  Pencil,
  Eye,
  Copy,
  Snowflake,
  Zap,
  Loader2,
  Camera,
  Download,
  Upload,
  Globe,
  ExternalLink,
  ArrowRight,
  Save,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, EmptyState, Modal } from '../components/ui';
import { save } from '@tauri-apps/plugin-dialog';
import { useAppStore } from '../store';
import { api } from '../lib/tauri';
import type { AccountView, GroupView, JwtParseResult, ProfileInfo } from '../types';

const PRESET_COLORS = [
  '#6366f1', '#22c55e', '#f59e0b', '#ef4444', '#0ea5e9', '#a855f7', '#14b8a6',
];

function JwtStatusBadge({ hours }: { hours: number | null }) {
  if (hours === null) return <Badge tone="slate"><HelpCircle size={12} /> 未知</Badge>;
  if (hours <= 0) return <Badge tone="red"><XCircle size={12} /> 已过期</Badge>;
  if (hours <= 24) return <Badge tone="amber"><AlertTriangle size={12} /> {hours.toFixed(1)}h</Badge>;
  return <Badge tone="green"><CheckCircle2 size={12} /> {hours.toFixed(0)}h</Badge>;
}

const COOLDOWN_LABELS: Record<string, string> = {
  PlanLimit: '套餐限额',
  SoftRate: '限流',
  SessionDead: '会话失效',
  NotFound: '接口异常',
  Server: '服务端错误',
  Client: '客户端错误',
  BusinessError: '业务错误',
};

function CooldownBadge({ type, until }: { type: string; until: number | null }) {
  const label = COOLDOWN_LABELS[type] ?? type;
  const isPermanent = type === 'SessionDead';
  let remaining = '';
  if (!isPermanent && until) {
    const secs = until - Math.floor(Date.now() / 1000);
    if (secs > 0) {
      const h = Math.floor(secs / 3600);
      const m = Math.floor((secs % 3600) / 60);
      remaining = h > 0 ? `${h}h${m}m` : `${m}m`;
    }
  }
  return (
    <Badge tone={isPermanent ? 'red' : 'amber'}>
      <Snowflake size={12} /> {label}{remaining && ` ${remaining}`}
    </Badge>
  );
}

function CreditsExpireBadge({ expireAt }: { expireAt: number | null }) {
  if (!expireAt) return <span className="text-xs text-slate-300">-</span>;
  const now = Math.floor(Date.now() / 1000);
  const secs = expireAt - now;
  if (secs <= 0) return <Badge tone="red">已过期</Badge>;
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const isUrgent = secs < 86400; // < 24h
  const text = days > 0 ? `${days}d${hours}h` : `${hours}h`;
  return (
    <span className={`text-xs ${isUrgent ? 'text-amber-500 font-semibold' : 'text-slate-500'}`}>
      {text}
    </span>
  );
}

export default function Accounts() {
  const accounts = useAppStore((s) => s.accounts);
  const groups = useAppStore((s) => s.groups);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const refreshGroups = useAppStore((s) => s.refreshGroups);
  const addAccount = useAppStore((s) => s.addAccount);
  const deleteAccount = useAppStore((s) => s.deleteAccount);
  const updateAccount = useAppStore((s) => s.updateAccount);
  const createGroup = useAppStore((s) => s.createGroup);
  const updateGroup = useAppStore((s) => s.updateGroup);
  const removeGroup = useAppStore((s) => s.removeGroup);
  const moveAccount = useAppStore((s) => s.moveAccount);
  const resetDevice = useAppStore((s) => s.resetDevice);
  const switchTo = useAppStore((s) => s.switchTo);
  const switchingTo = useAppStore((s) => s.switchingTo);
  const saveCurrentLogin = useAppStore((s) => s.saveCurrentLogin);
  const savingLogin = useAppStore((s) => s.savingLogin);
  const renewJwt = useAppStore((s) => s.renewJwt);
  const refreshRemainingCredits = useAppStore((s) => s.refreshRemainingCredits);
  const cooldownClear = useAppStore((s) => s.cooldownClear);
  const refreshJwt = useAppStore((s) => s.refreshJwt);
  const toast = useAppStore((s) => s.pushToast);
  const profiles = useAppStore((s) => s.profiles);
  const profileProgress = useAppStore((s) => s.profileProgress);
  const profileActive = useAppStore((s) => s.profileActive);
  const profileBackup = useAppStore((s) => s.profileBackup);
  const profileRestore = useAppStore((s) => s.profileRestore);
  const profileDelete = useAppStore((s) => s.profileDelete);
  const oauthLogin = useAppStore((s) => s.oauthLogin);
  const refreshProfiles = useAppStore((s) => s.refreshProfiles);

  const [filter, setFilter] = useState<string>('all');
  const [addOpen, setAddOpen] = useState(false);
  const [groupOpen, setGroupOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<AccountView | null>(null);
  const [jwtTarget, setJwtTarget] = useState<AccountView | null>(null);
  const [profileOpen, setProfileOpen] = useState(false);
  const [oauthOpen, setOAuthOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);

  const filtered = useMemo(() => {
    if (filter === 'all') return accounts;
    if (filter === 'ungrouped') return accounts.filter((a) => !a.group_id);
    return accounts.filter((a) => a.group_id === filter);
  }, [accounts, filter]);

  useEffect(() => {
    void refreshAccounts();
    void refreshGroups();
  }, [refreshAccounts, refreshGroups]);

  const onDelete = async (a: AccountView) => {
    if (!confirm(`确认删除账号「${a.name}」？${a.device_id_masked ? '（会一并清理设备 ID）' : ''}`)) return;
    await deleteAccount(a.user_id, true);
  };

  const copyJwt = async (jwt: string) => {
    try {
      await navigator.clipboard.writeText(jwt);
      toast('success', 'JWT 已复制到剪贴板');
    } catch {
      toast('error', '复制失败，请手动选择文本复制');
    }
  };

  const exportAccounts = async () => {
    if (accounts.length === 0) {
      toast('warn', '没有账号可导出');
      return;
    }
    try {
      const payload = await api.accounts.exportRaw();
      const content = JSON.stringify(payload, null, 2);
      const fileStamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, '-');
      const filePath = await save({
        defaultPath: `trae-accounts-${fileStamp}.json`,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!filePath) return;
      await api.misc.writeTextFile(filePath, content);
      toast('success', `已导出 ${accounts.length} 个账号到 ${filePath}`);
    } catch (err) {
      toast('error', `导出失败：${String(err)}`);
    }
  };

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="账号管理"
        desc="维护账号、调整分组、重置设备 ID 与登录态切换"
        actions={
          <>
            <button onClick={() => void exportAccounts()} className="btn-outline" title="导出所有账号为 JSON 文件">
              <Download size={15} /> 导出
            </button>
            <button onClick={() => setHelpOpen(true)} className="btn-outline" title="使用帮助">
              <HelpCircle size={15} /> 帮助
            </button>
            <button onClick={() => { void refreshAccounts(); void refreshGroups(); void refreshRemainingCredits(); }} className="btn-outline">
              <RefreshCw size={15} /> 刷新
            </button>
            <button onClick={() => setGroupOpen(true)} className="btn-outline">
              分组管理
            </button>
            <button onClick={() => { void refreshProfiles(); setProfileOpen(true); }} className="btn-outline">
              <Camera size={15} /> 快照管理
            </button>
            <button onClick={() => setOAuthOpen(true)} className="btn-outline">
              <Globe size={15} /> OAuth 登录
            </button>
            <button onClick={() => setAddOpen(true)} className="btn-primary">
              <Plus size={15} /> 添加账号
            </button>
          </>
        }
      />

      <div className="mb-3 flex flex-wrap items-center gap-2 text-sm">
        <button
          onClick={() => setFilter('all')}
          className={`chip border ${filter === 'all' ? 'border-brand-500 text-brand-600' : 'border-slate-300 text-slate-500 dark:border-zinc-700 dark:text-zinc-400'}`}
        >
          全部 ({accounts.length})
        </button>
        <button
          onClick={() => setFilter('ungrouped')}
          className={`chip border ${filter === 'ungrouped' ? 'border-brand-500 text-brand-600' : 'border-slate-300 text-slate-500 dark:border-zinc-700 dark:text-zinc-400'}`}
        >
          未分组 ({accounts.filter((a) => !a.group_id).length})
        </button>
        {groups.map((g) => (
          <button
            key={g.id}
            onClick={() => setFilter(g.id)}
            className={`chip border ${filter === g.id ? 'border-brand-500 text-brand-600' : 'border-slate-300 text-slate-500 dark:border-zinc-700 dark:text-zinc-400'}`}
            style={{ borderColor: filter === g.id ? g.color : undefined }}
          >
            <span className="inline-block h-2 w-2 rounded-full" style={{ background: g.color }} />
            {g.name} ({g.count})
          </button>
        ))}
      </div>

      <div className="card overflow-hidden">
        {filtered.length === 0 ? (
          <div className="p-6">
            <EmptyState
              icon={<Plus size={28} />}
              title={filter === 'all' ? '还没有账号' : '此分组下没有账号'}
              hint="点击右上角「添加账号」粘贴 JWT，或先启动代理让 TRAE 自动捕获。"
            />
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
              <tr>
                <th className="px-4 py-2 text-left">账号</th>
                <th className="px-4 py-2 text-left">分组</th>
                <th className="px-4 py-2 text-left">JWT</th>
                <th className="px-4 py-2 text-left">设备 ID</th>
                <th className="px-4 py-2 text-left">今日</th>
                <th className="px-4 py-2 text-left">冷却</th>
                <th className="px-4 py-2 text-right">剩余积分</th>
                <th className="px-4 py-2 text-left">积分过期</th>
                <th className="px-4 py-2 text-right">今日新增积分</th>
                <th className="px-4 py-2 text-right">操作</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((a) => {
                return (
                  <tr key={a.user_id} className="border-t border-slate-200 dark:border-zinc-800">
                    <td className="px-4 py-3">
                      <div className="font-medium">{a.name}</div>
                      <div className="text-xs text-slate-400">{a.user_id}</div>
                    </td>
                    <td className="px-4 py-3">
                      <GroupSelect
                        value={a.group_id}
                        groups={groups}
                        onChange={(gid) => void moveAccount(a.user_id, gid)}
                      />
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-1">
                        <JwtStatusBadge hours={a.jwt_exp_hours} />
                        {a.has_refresh_token && (
                          <span title="支持自动刷新" className="text-sky-500">
                            <Zap size={12} />
                          </span>
                        )}
                        <button
                          title="查看 JWT"
                          onClick={() => setJwtTarget(a)}
                          className="btn-ghost !p-1"
                        >
                          <Eye size={13} />
                        </button>
                      </div>
                    </td>
                    <td className="px-4 py-3 font-mono text-xs text-slate-500">{a.device_id_masked ?? '-'}</td>
                    <td className="px-4 py-3">
                      {a.checked_today ? (
                        <Badge tone="green">已签</Badge>
                      ) : (
                        <Badge tone="slate">未签</Badge>
                      )}
                    </td>
                    <td className="px-4 py-3">
                      {a.cooldown_type ? (
                        <CooldownBadge type={a.cooldown_type} until={a.cooldown_until} />
                      ) : (
                        <span className="text-xs text-slate-300">-</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-right tabular-nums">
                      {a.remaining_credits != null
                        ? a.remaining_credits.toLocaleString('zh-CN', { minimumFractionDigits: 0, maximumFractionDigits: 2 })
                        : '-'}
                    </td>
                    <td className="px-4 py-3">
                      <CreditsExpireBadge expireAt={a.credits_expire_at} />
                    </td>
                    <td className="px-4 py-3 text-right tabular-nums">
                      {a.credits != null ? a.credits.toLocaleString() : '-'}
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1">
                        <button title="编辑账号" onClick={() => setEditTarget(a)} className="btn-ghost !p-2">
                          <Pencil size={14} />
                        </button>
                        {a.cooldown_type && (
                          <button
                            title="解除冷却"
                            onClick={() => void cooldownClear(a.user_id)}
                            className="btn-ghost !p-2 text-sky-500 hover:bg-sky-50 dark:hover:bg-sky-500/10"
                          >
                            <Snowflake size={14} />
                          </button>
                        )}
                        {(a.jwt_exp_hours === null || a.jwt_exp_hours <= 24) && (
                          <button
                            title="续期 JWT（启动代理并切换账号）"
                            onClick={() => void renewJwt(a.user_id)}
                            className="btn-ghost !p-2 text-amber-500 hover:bg-amber-50 dark:hover:bg-amber-500/10"
                          >
                            <KeyRound size={14} />
                          </button>
                        )}
                        {a.has_refresh_token && (
                          <button
                            title="刷新 JWT"
                            onClick={() => void refreshJwt(a.user_id)}
                            className="btn-ghost !p-2 text-sky-500 hover:bg-sky-50 dark:hover:bg-sky-500/10"
                          >
                            <Zap size={14} />
                          </button>
                        )}
                        <button
                          title={switchingTo ? (switchingTo === a.user_id ? '切换中…' : '正在切换其他账号') : '切换到此账号'}
                          onClick={() => void switchTo(a.user_id)}
                          disabled={!!switchingTo || !!savingLogin}
                          className={`btn-ghost !p-2 ${switchingTo === a.user_id ? 'text-amber-500' : ''} ${(switchingTo && switchingTo !== a.user_id) || savingLogin ? 'opacity-40 cursor-not-allowed' : ''}`}
                        >
                          {switchingTo === a.user_id ? <Loader2 size={14} className="animate-spin" /> : <LogIn size={14} />}
                        </button>
                        <button
                          title={savingLogin ? (savingLogin === a.user_id ? '保存中…' : '正在保存其他账号') : '保存当前登录态'}
                          onClick={() => void saveCurrentLogin(a.user_id)}
                          disabled={!!switchingTo || !!savingLogin}
                          className={`btn-ghost !p-2 ${savingLogin === a.user_id ? 'text-amber-500' : ''} ${(savingLogin && savingLogin !== a.user_id) || switchingTo ? 'opacity-40 cursor-not-allowed' : ''}`}
                        >
                          {savingLogin === a.user_id ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
                        </button>
                        <button title="重置设备 ID" onClick={() => void resetDevice(a.user_id)} className="btn-ghost !p-2">
                          <RotateCcw size={14} />
                        </button>
                        <button title="删除" onClick={() => void onDelete(a)} className="btn-ghost !p-2 text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-500/10">
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

      <AddAccountModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        groups={groups}
        onSubmit={async (name, jwt, gid) => {
          try {
            await addAccount(name, jwt, gid);
            setAddOpen(false);
          } catch {
            /* toast 已发出 */
          }
        }}
      />
      <EditAccountModal
        account={editTarget}
        onClose={() => setEditTarget(null)}
        onSubmit={async (name, jwt) => {
          if (!editTarget) return;
          try {
            await updateAccount(editTarget.user_id, name, jwt);
            setEditTarget(null);
          } catch {
            /* toast 已发出 */
          }
        }}
      />
      <JwtViewModal
        account={jwtTarget}
        onClose={() => setJwtTarget(null)}
        onCopy={copyJwt}
      />
      <GroupsModal
        open={groupOpen}
        onClose={() => setGroupOpen(false)}
        groups={groups}
        onCreate={async (name, color) => {
          await createGroup(name, color);
        }}
        onRename={async (id, name) => {
          await updateGroup(id, { name });
        }}
        onRecolor={async (id, color) => {
          await updateGroup(id, { color });
        }}
        onDelete={async (id) => {
          await removeGroup(id);
        }}
      />
      <ProfileModal
        open={profileOpen}
        onClose={() => setProfileOpen(false)}
        profiles={profiles}
        profileActive={profileActive}
        profileProgress={profileProgress}
        onBackup={(slot) => void profileBackup(slot)}
        onRestore={(slot) => void profileRestore(slot)}
        onDelete={async (slot) => {
          if (!confirm(`确认删除快照「${slot}」？`)) return;
          await profileDelete(slot);
        }}
      />
      <OAuthLoginModal
        open={oauthOpen}
        onClose={() => setOAuthOpen(false)}
        groups={groups}
        onLogin={async (callbackUrl, accountName, groupId) => {
          try {
            await oauthLogin(callbackUrl, accountName, groupId);
            setOAuthOpen(false);
          } catch {
            /* toast 已发出 */
          }
        }}
      />
      <HelpModal open={helpOpen} onClose={() => setHelpOpen(false)} />
    </div>
  );
}

function GroupSelect({
  value,
  groups,
  onChange,
}: {
  value: string | null;
  groups: GroupView[];
  onChange: (gid: string | null) => void;
}) {
  return (
    <select
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value || null)}
      className="input !py-1 !text-xs w-32"
    >
      <option value="">未分组</option>
      {groups.map((g) => (
        <option key={g.id} value={g.id}>
          {g.name}
        </option>
      ))}
    </select>
  );
}

function AddAccountModal({
  open,
  onClose,
  groups,
  onSubmit,
}: {
  open: boolean;
  onClose: () => void;
  groups: GroupView[];
  onSubmit: (name: string, jwt: string, groupId?: string) => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [jwt, setJwt] = useState('');
  const [gid, setGid] = useState<string>('');
  const [info, setInfo] = useState<JwtParseResult | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setName('');
      setJwt('');
      setGid('');
      setInfo(null);
      setBusy(false);
    }
  }, [open]);

  // 输入 JWT 时自动解析
  useEffect(() => {
    const v = jwt.trim();
    if (!v) {
      setInfo(null);
      return;
    }
    let cancel = false;
    const t = setTimeout(async () => {
      try {
        const r = await api.misc.jwtParse(v);
        if (!cancel) setInfo(r);
      } catch {
        if (!cancel) setInfo({ user_id: null, exp_hours: null, exp_timestamp: null, status: 'unknown' });
      }
    }, 200);
    return () => {
      cancel = true;
      clearTimeout(t);
    };
  }, [jwt]);

  const submit = async () => {
    if (!name.trim() || !jwt.trim()) return;
    setBusy(true);
    try {
      await onSubmit(name.trim(), jwt.trim(), gid || undefined);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="添加账号"
      footer={
        <>
          <button onClick={onClose} className="btn-ghost">取消</button>
          <button onClick={submit} disabled={busy || !name || !jwt} className="btn-primary">
            {busy ? '添加中…' : '添加'}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <div>
          <label className="label">账号备注名</label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="input"
            placeholder="例如：me_1676"
          />
        </div>
        <div>
          <label className="label">JWT（支持「Cloud-IDE-JWT 」/「Bearer 」前缀，或直接粘贴 token）</label>
          <textarea
            value={jwt}
            onChange={(e) => setJwt(e.target.value)}
            className="input min-h-[120px] font-mono text-xs"
            placeholder="eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9..."
          />
        </div>
        <div>
          <label className="label">分组（可选）</label>
          <select value={gid} onChange={(e) => setGid(e.target.value)} className="input">
            <option value="">不分组</option>
            {groups.map((g) => (
              <option key={g.id} value={g.id}>{g.name}</option>
            ))}
          </select>
        </div>
        {info && (
          <div className="rounded-lg border border-slate-200 p-3 text-xs dark:border-zinc-700">
            <div>解析结果：</div>
            <div className="mt-1 grid grid-cols-2 gap-1">
              <span className="text-slate-500">UserID</span>
              <span className="font-mono">{info.user_id ?? '无法识别'}</span>
              <span className="text-slate-500">剩余</span>
              <span>{info.exp_hours != null ? `${info.exp_hours.toFixed(1)} 小时` : '-'}</span>
              <span className="text-slate-500">过期时间</span>
              <span>{info.exp_timestamp != null ? new Date(info.exp_timestamp * 1000).toLocaleString('zh-CN') : '-'}</span>
              <span className="text-slate-500">状态</span>
              <span>
                {info.status === 'ok' && '✅ 健康'}
                {info.status === 'warn' && '⚠️ 即将过期'}
                {info.status === 'expired' && '❌ 已过期'}
                {info.status === 'unknown' && '❓ 无法解析'}
              </span>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}

function EditAccountModal({
  account,
  onClose,
  onSubmit,
}: {
  account: AccountView | null;
  onClose: () => void;
  onSubmit: (name?: string, jwt?: string) => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [jwt, setJwt] = useState('');
  const [info, setInfo] = useState<JwtParseResult | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (account) {
      setName(account.name);
      setJwt('');
      setInfo(null);
      setBusy(false);
    }
  }, [account]);

  useEffect(() => {
    const v = jwt.trim();
    if (!v) {
      setInfo(null);
      return;
    }
    let cancel = false;
    const t = setTimeout(async () => {
      try {
        const r = await api.misc.jwtParse(v);
        if (!cancel) setInfo(r);
      } catch {
        if (!cancel) setInfo({ user_id: null, exp_hours: null, exp_timestamp: null, status: 'unknown' });
      }
    }, 200);
    return () => {
      cancel = true;
      clearTimeout(t);
    };
  }, [jwt]);

  const submit = async () => {
    if (!account) return;
    const nameChanged = name.trim() && name.trim() !== account.name;
    const jwtChanged = jwt.trim().length > 0;
    if (!nameChanged && !jwtChanged) {
      onClose();
      return;
    }
    setBusy(true);
    try {
      await onSubmit(
        nameChanged ? name.trim() : undefined,
        jwtChanged ? jwt.trim() : undefined,
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={!!account}
      onClose={onClose}
      title="编辑账号"
      footer={
        <>
          <button onClick={onClose} className="btn-ghost">取消</button>
          <button onClick={submit} disabled={busy} className="btn-primary">
            {busy ? '保存中…' : '保存'}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <div>
          <label className="label">账号备注名</label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="input"
            placeholder="例如：me_1676"
          />
        </div>
        <div>
          <label className="label">新 JWT（留空则不修改）</label>
          <textarea
            value={jwt}
            onChange={(e) => setJwt(e.target.value)}
            className="input min-h-[120px] font-mono text-xs"
            placeholder="粘贴新 JWT 以替换（支持 Cloud-IDE-JWT / Bearer 前缀或直接粘贴 token）"
          />
        </div>
        {info && (
          <div className="rounded-lg border border-slate-200 p-3 text-xs dark:border-zinc-700">
            <div>新 JWT 解析结果：</div>
            <div className="mt-1 grid grid-cols-2 gap-1">
              <span className="text-slate-500">UserID</span>
              <span className="font-mono">{info.user_id ?? '无法识别'}</span>
              <span className="text-slate-500">剩余</span>
              <span>{info.exp_hours != null ? `${info.exp_hours.toFixed(1)} 小时` : '-'}</span>
              <span className="text-slate-500">过期时间</span>
              <span>{info.exp_timestamp != null ? new Date(info.exp_timestamp * 1000).toLocaleString('zh-CN') : '-'}</span>
              <span className="text-slate-500">状态</span>
              <span>
                {info.status === 'ok' && '✅ 健康'}
                {info.status === 'warn' && '⚠️ 即将过期'}
                {info.status === 'expired' && '❌ 已过期'}
                {info.status === 'unknown' && '❓ 无法解析'}
              </span>
            </div>
          </div>
        )}
        {jwt.trim() && info && info.user_id && account && info.user_id !== account.user_id && (
          <div className="rounded-lg border border-amber-300 bg-amber-50 p-3 text-xs text-amber-700 dark:border-amber-700 dark:bg-amber-900/20 dark:text-amber-300">
            ⚠️ 新 JWT 的 UserID（{info.user_id}）与当前账号（{account.user_id}）不同，保存后 user_id 将更新。
          </div>
        )}
      </div>
    </Modal>
  );
}

function JwtViewModal({
  account,
  onClose,
  onCopy,
}: {
  account: AccountView | null;
  onClose: () => void;
  onCopy: (jwt: string) => void;
}) {
  if (!account) return null;
  return (
    <Modal
      open={!!account}
      onClose={onClose}
      title={`JWT - ${account.name}`}
      footer={
        <>
          <button onClick={onClose} className="btn-ghost">关闭</button>
          <button
            onClick={() => onCopy(account.jwt)}
            className="btn-primary"
          >
            <Copy size={14} /> 复制 JWT
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <div className="grid grid-cols-2 gap-2 text-xs">
          <span className="text-slate-500">UserID</span>
          <span className="font-mono">{account.user_id}</span>
          <span className="text-slate-500">JWT 剩余</span>
          <span>
            {account.jwt_exp_hours != null
              ? `${account.jwt_exp_hours.toFixed(1)} 小时`
              : '未知'}
          </span>
          <span className="text-slate-500">过期时间</span>
          <span>
            {account.jwt_exp_timestamp != null
              ? new Date(account.jwt_exp_timestamp * 1000).toLocaleString('zh-CN')
              : '未知'}
          </span>
        </div>
        <div>
          <label className="label">JWT 原文</label>
          <textarea
            readOnly
            value={account.jwt}
            className="input min-h-[160px] font-mono text-xs"
            onClick={(e) => (e.target as HTMLTextAreaElement).select()}
          />
        </div>
      </div>
    </Modal>
  );
}

function GroupsModal({
  open,
  onClose,
  groups,
  onCreate,
  onRename,
  onRecolor,
  onDelete,
}: {
  open: boolean;
  onClose: () => void;
  groups: GroupView[];
  onCreate: (name: string, color: string) => Promise<void>;
  onRename: (id: string, name: string) => Promise<void>;
  onRecolor: (id: string, color: string) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [color, setColor] = useState(PRESET_COLORS[0]);
  const [editingNames, setEditingNames] = useState<Record<string, string>>({});

  useEffect(() => {
    setEditingNames((prev) => {
      const next = { ...prev };
      groups.forEach((g) => {
        if (!(g.id in next)) next[g.id] = g.name;
      });
      return next;
    });
  }, [groups]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="分组管理"
      footer={
        <>
          <button onClick={onClose} className="btn-ghost">关闭</button>
          <button
            onClick={async () => {
              if (!name.trim()) return;
              await onCreate(name.trim(), color);
              setName('');
            }}
            className="btn-primary"
          >
            <Plus size={14} /> 新建
          </button>
        </>
      }
    >
      <div className="mb-4 space-y-3">
        <div>
          <label className="label">新分组名称</label>
          <input value={name} onChange={(e) => setName(e.target.value)} className="input" placeholder="例如：工作 / 私人" />
        </div>
        <div>
          <label className="label">颜色</label>
          <div className="flex flex-wrap gap-2">
            {PRESET_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                onClick={() => setColor(c)}
                className={`h-6 w-6 rounded-full border-2 ${color === c ? 'border-slate-900 dark:border-white' : 'border-transparent'}`}
                style={{ background: c }}
                aria-label={c}
              />
            ))}
          </div>
        </div>
      </div>
      <div className="max-h-64 space-y-2 overflow-auto">
        {groups.length === 0 && <div className="text-xs text-slate-400">暂无分组</div>}
        {groups.map((g) => (
          <div key={g.id} className="flex items-center gap-2 rounded-lg border border-slate-200 p-2 dark:border-zinc-700">
            <span className="inline-block h-4 w-4 rounded-full" style={{ background: g.color }} />
            <input
              value={editingNames[g.id] ?? g.name}
              onChange={(e) =>
                setEditingNames((prev) => ({ ...prev, [g.id]: e.target.value }))
              }
              onBlur={(e) => {
                const val = e.target.value.trim();
                if (val && val !== g.name) {
                  void onRename(g.id, val).then(() => {
                    setEditingNames((prev) => {
                      const next = { ...prev };
                      delete next[g.id];
                      return next;
                    });
                  });
                }
              }}
              className="input !py-1 flex-1 !text-xs"
            />
            <select
              value={g.color}
              onChange={(e) => void onRecolor(g.id, e.target.value)}
              className="input !py-1 !text-xs w-24"
            >
              {PRESET_COLORS.map((c) => (
                <option key={c} value={c}>{c}</option>
              ))}
            </select>
            <span className="text-xs text-slate-400">{g.count}</span>
            <button
              onClick={() => {
                if (confirm(`删除分组「${g.name}」？该分组下账号会回落为「未分组」。`))
                  void onDelete(g.id);
              }}
              className="btn-ghost !p-2 text-rose-500"
              aria-label="删除"
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
      </div>
    </Modal>
  );
}

function ProfileSize({ bytes }: { bytes: number }) {
  const [text, setText] = useState('');
  useEffect(() => {
    let cancel = false;
    api.profiles
      .formatSize(bytes)
      .then((t) => {
        if (!cancel) setText(t);
      })
      .catch(() => {
        if (!cancel) setText(`${bytes} B`);
      });
    return () => {
      cancel = true;
    };
  }, [bytes]);
  return <span>{text || '...'}</span>;
}

function ProfileModal({
  open,
  onClose,
  profiles,
  profileActive,
  profileProgress,
  onBackup,
  onRestore,
  onDelete,
}: {
  open: boolean;
  onClose: () => void;
  profiles: ProfileInfo[];
  profileActive: boolean;
  profileProgress: string[];
  onBackup: (slot: string) => void;
  onRestore: (slot: string) => void;
  onDelete: (slot: string) => Promise<void>;
}) {
  return (
    <Modal open={open} onClose={onClose} title="登录态快照管理" size="xl">
      <div className="space-y-3">
        {profileActive && (
          <div className="rounded-lg border border-brand-300 bg-brand-50 p-3 dark:border-brand-700 dark:bg-brand-900/20">
            <div className="mb-1 text-xs font-medium text-brand-700 dark:text-brand-300">
              正在处理...
            </div>
            <div className="max-h-32 space-y-0.5 overflow-auto font-mono text-xs text-brand-600 dark:text-brand-400">
              {profileProgress.length === 0 ? (
                <div>等待中...</div>
              ) : (
                profileProgress.map((line, i) => <div key={i}>{line}</div>)
              )}
            </div>
          </div>
        )}
        {profiles.length === 0 ? (
          <div className="py-6 text-center text-xs text-slate-400">暂无快照</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
              <tr>
                <th className="whitespace-nowrap px-3 py-2 text-left">账号 (user_id)</th>
                <th className="whitespace-nowrap px-3 py-2 text-right">文件数</th>
                <th className="whitespace-nowrap px-3 py-2 text-right">大小</th>
                <th className="whitespace-nowrap px-3 py-2 text-left">最后修改</th>
                <th className="whitespace-nowrap px-3 py-2 text-right">操作</th>
              </tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <tr
                  key={p.slot}
                  className="border-t border-slate-200 dark:border-zinc-800"
                >
                  <td className="whitespace-nowrap px-3 py-2 font-mono text-xs">{p.slot}</td>
                  <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">{p.file_count}</td>
                  <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
                    <ProfileSize bytes={p.size_bytes} />
                  </td>
                  <td className="whitespace-nowrap px-3 py-2 text-xs text-slate-500">
                    {p.last_modified || '-'}
                  </td>
                  <td className="whitespace-nowrap px-3 py-2">
                    <div className="flex justify-end gap-1">
                      <button
                        title="备份"
                        onClick={() => onBackup(p.slot)}
                        disabled={profileActive}
                        className="btn-ghost !p-2"
                      >
                        <Upload size={14} />
                      </button>
                      <button
                        title="恢复"
                        onClick={() => onRestore(p.slot)}
                        disabled={profileActive}
                        className="btn-ghost !p-2 text-sky-500 hover:bg-sky-50 dark:hover:bg-sky-500/10"
                      >
                        <Download size={14} />
                      </button>
                      <button
                        title="删除"
                        onClick={() => void onDelete(p.slot)}
                        disabled={profileActive}
                        className="btn-ghost !p-2 text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-500/10"
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <div className="text-xs text-slate-400">
          快照在切换账号时自动备份，也可手动管理
        </div>
      </div>
    </Modal>
  );
}

function OAuthLoginModal({
  open,
  onClose,
  groups,
  onLogin,
}: {
  open: boolean;
  onClose: () => void;
  groups: GroupView[];
  onLogin: (
    callbackUrl: string,
    accountName?: string,
    groupId?: string,
  ) => Promise<void>;
}) {
  const [step, setStep] = useState(1);
  const [callbackUrl, setCallbackUrl] = useState('');
  const [accountName, setAccountName] = useState('');
  const [gid, setGid] = useState('');
  const [busy, setBusy] = useState(false);
  const [opening, setOpening] = useState(false);
  const toast = useAppStore((s) => s.pushToast);

  useEffect(() => {
    if (!open) {
      setStep(1);
      setCallbackUrl('');
      setAccountName('');
      setGid('');
      setBusy(false);
      setOpening(false);
    }
  }, [open]);

  // 监听本地回调服务器推送的授权结果：官网完成授权后会重定向到
  // 127.0.0.1:17388/authorize，后端接住并通过 oauth-callback 事件
  // 把完整回调 URL 发到这里，自动填充并进入下一步
  useEffect(() => {
    if (!open) return;
    let un: (() => void) | undefined;
    let alive = true;
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen<string>('oauth-callback', (e) => {
        if (!e.payload) return;
        setCallbackUrl(e.payload);
        setStep(3);
        toast('success', '已接收 OAuth 回调，请确认并完成登录');
      }).then((f) => {
        if (alive) {
          un = f;
        } else {
          f();
        }
      }),
    );
    return () => {
      alive = false;
      un?.();
    };
  }, [open, toast]);

  // 关闭弹窗时停止本地回调服务器，释放 17388 端口
  useEffect(() => {
    if (open) return;
    void api.oauth.callbackStop().catch(() => {});
  }, [open]);

  const openLoginPage = async () => {
    setOpening(true);
    try {
      // 先启动本地回调监听：官网授权页会探测本地 17388 端口确认
      // "客户端在线"，没有监听会一直卡在"认证中，正在验证身份"
      try {
        await api.oauth.callbackStart();
      } catch (err) {
        toast(
          'warn',
          `本地回调监听启动失败：${String(err)}。若官网长时间卡在"认证中"，请关闭占用 17388 端口的程序后重试`,
        );
      }
      const { url } = await api.oauth.getLoginUrl();
      const { open } = await import('@tauri-apps/plugin-shell');
      await open(url);
      setStep(2);
    } catch (err) {
      toast('error', `获取登录 URL 失败：${String(err)}`);
    } finally {
      setOpening(false);
    }
  };

  const finish = async () => {
    if (!callbackUrl.trim()) return;
    setBusy(true);
    try {
      await onLogin(
        callbackUrl.trim(),
        accountName.trim() || undefined,
        gid || undefined,
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="OAuth 登录"
      footer={
        <>
          {step > 1 && (
            <button
              onClick={() => setStep((s) => Math.max(1, s - 1))}
              className="btn-ghost"
              disabled={busy || opening}
            >
              上一步
            </button>
          )}
          <button onClick={onClose} className="btn-ghost" disabled={busy || opening}>
            取消
          </button>
          {step === 1 && (
            <button
              onClick={openLoginPage}
              disabled={opening}
              className="btn-primary"
            >
              {opening ? '正在打开...' : '打开登录页'}
              {!opening && <ExternalLink size={14} />}
            </button>
          )}
          {step === 2 && (
            <button
              onClick={() => setStep(3)}
              disabled={!callbackUrl.trim()}
              className="btn-primary"
            >
              下一步 <ArrowRight size={14} />
            </button>
          )}
          {step === 3 && (
            <button
              onClick={finish}
              disabled={busy || !callbackUrl.trim()}
              className="btn-primary"
            >
              {busy ? '登录中...' : '完成登录'}
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
        {/* 步骤指示器 */}
        <div className="flex items-center gap-2">
          <div
            className={`flex h-7 w-7 items-center justify-center rounded-full text-xs font-semibold ${
              step >= 1
                ? 'bg-brand-500 text-white'
                : 'bg-slate-200 text-slate-500 dark:bg-zinc-700'
            }`}
          >
            1
          </div>
          <div
            className={`h-0.5 w-8 ${step > 1 ? 'bg-brand-500' : 'bg-slate-200 dark:bg-zinc-700'}`}
          />
          <div
            className={`flex h-7 w-7 items-center justify-center rounded-full text-xs font-semibold ${
              step >= 2
                ? 'bg-brand-500 text-white'
                : 'bg-slate-200 text-slate-500 dark:bg-zinc-700'
            }`}
          >
            2
          </div>
          <div
            className={`h-0.5 w-8 ${step > 2 ? 'bg-brand-500' : 'bg-slate-200 dark:bg-zinc-700'}`}
          />
          <div
            className={`flex h-7 w-7 items-center justify-center rounded-full text-xs font-semibold ${
              step >= 3
                ? 'bg-brand-500 text-white'
                : 'bg-slate-200 text-slate-500 dark:bg-zinc-700'
            }`}
          >
            3
          </div>
        </div>

        {step === 1 && (
          <div className="text-sm text-slate-600 dark:text-zinc-300">
            点击「打开登录页」在浏览器中发起 OAuth 登录，完成后将自动进入下一步。
          </div>
        )}

        {step === 2 && (
          <div>
            <div className="mb-3 flex items-center gap-2 text-xs text-slate-500 dark:text-zinc-400">
              <Loader2 size={13} className="animate-spin" />
              正在等待浏览器完成授权，回调将自动填入…
            </div>
            <label className="label">回调 URL</label>
            <textarea
              value={callbackUrl}
              onChange={(e) => setCallbackUrl(e.target.value)}
              className="input min-h-[100px] font-mono text-xs"
              placeholder="http://127.0.0.1:17388/authorize?code=..."
            />
            <p className="mt-2 text-xs text-slate-400">
              授权完成后会自动填入。若浏览器未跳转或自动填充失败，可将地址栏完整 URL 手动复制粘贴到此处
            </p>
          </div>
        )}

        {step === 3 && (
          <div className="space-y-3">
            <div>
              <label className="label">账号备注名（可选）</label>
              <input
                value={accountName}
                onChange={(e) => setAccountName(e.target.value)}
                className="input"
                placeholder="例如：me_1676"
              />
            </div>
            <div>
              <label className="label">分组（可选）</label>
              <select
                value={gid}
                onChange={(e) => setGid(e.target.value)}
                className="input"
              >
                <option value="">不分组</option>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name}
                  </option>
                ))}
              </select>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}

function HelpModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Modal open={open} onClose={onClose} title="账号管理使用帮助" size="xl">
      <div className="space-y-4 text-sm">
        <section className="rounded-lg border border-amber-200 bg-amber-50 p-3 dark:border-amber-700 dark:bg-amber-900/20">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold text-amber-700 dark:text-amber-300">
            <Globe size={15} /> OAuth 登录（自动保存账号）
          </h3>
          <p className="text-xs leading-relaxed text-amber-800 dark:text-amber-200">
            点击「OAuth 登录」按钮，在浏览器中完成 Trae Work 账号登录。登录完成后将回调 URL 粘贴回应用，
            系统会自动解析 JWT 并保存账号信息，无需手动粘贴 token。适合首次添加账号或 JWT 过期后重新登录。
          </p>
        </section>

        <section className="rounded-lg border border-slate-200 p-3 dark:border-zinc-700">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold">
            <Save size={15} className="text-amber-500" /> 保存当前登录态
          </h3>
          <p className="text-xs leading-relaxed text-slate-600 dark:text-zinc-300">
            在 Trae Work 中登录某个账号后，点击该账号行的「保存」图标，系统会关闭 Trae Work → 精准备份 9 类核心登录文件
            （storage.json、state.vscdb、machineid、aha、Network 等）→ 重新启动 Trae Work。
            每个账号的登录态独立存储，互不干扰。
          </p>
          <p className="mt-1 text-xs font-medium text-amber-600 dark:text-amber-400">
            ⚠ 首次使用前，请先在 Trae Work 中登录目标账号，然后点击「保存」图标创建快照。
          </p>
        </section>

        <section className="rounded-lg border border-slate-200 p-3 dark:border-zinc-700">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold">
            <LogIn size={15} className="text-amber-500" /> 切换账号流程
          </h3>
          <ol className="ml-4 list-decimal space-y-1 text-xs leading-relaxed text-slate-600 dark:text-zinc-300">
            <li>点击目标账号行的「切换」图标</li>
            <li>系统自动保存当前登录态到当前账号槽位（如果已知当前账号 ID）</li>
            <li>同时备份到 <code className="rounded bg-slate-100 px-1 dark:bg-zinc-800">last</code> 槽位作为安全回退</li>
            <li>恢复目标账号的登录态（含设备标识）</li>
            <li>重新启动 Trae Work，自动以目标账号登录</li>
          </ol>
          <p className="mt-1 text-xs font-medium text-amber-600 dark:text-amber-400">
            ⚠ 如果目标账号从未保存过登录态，切换会被中止并提示「无快照」。请先用「保存」图标创建快照。
          </p>
        </section>

        <section className="rounded-lg border border-slate-200 p-3 dark:border-zinc-700">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold">
            <Camera size={15} className="text-amber-500" /> 快照管理
          </h3>
          <p className="text-xs leading-relaxed text-slate-600 dark:text-zinc-300">
            点击「快照管理」按钮可查看所有已保存的登录态快照。每个快照以账号 UserID 命名，
            显示文件数、大小和最后修改时间。支持手动备份、恢复和删除操作。
            <code className="rounded bg-slate-100 px-1 dark:bg-zinc-800">last</code> 槽位是切换时自动创建的安全备份。
          </p>
        </section>

        <section className="rounded-lg border border-slate-200 p-3 dark:border-zinc-700">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold">
            <RotateCcw size={15} className="text-amber-500" /> 重置设备 ID
          </h3>
          <p className="text-xs leading-relaxed text-slate-600 dark:text-zinc-300">
            点击账号行的「重置」图标可重置该账号的设备 ID（用于解决设备绑定问题）。
            如需全局重置 6 层设备标识（machineid、storage.json、aha、注册表 MachineGuid 等），
            请到设置页面执行「6 层设备标识重置」。
          </p>
        </section>

        <section className="rounded-lg border border-slate-200 p-3 dark:border-zinc-700">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold">
            <Zap size={15} className="text-amber-500" /> 代理自动抓取账号
          </h3>
          <p className="text-xs leading-relaxed text-slate-600 dark:text-zinc-300">
            启动代理服务后，在 Trae 中登录任何账号，代理会自动捕获 JWT token 并保存到账号列表中。
            无需手动粘贴 JWT，适合批量导入账号。捕获的账号会自动解析 UserID、过期时间等信息。
          </p>
        </section>

        <section className="rounded-lg border border-sky-200 bg-sky-50 p-3 dark:border-sky-700 dark:bg-sky-900/20">
          <h3 className="mb-1 flex items-center gap-1.5 font-semibold text-sky-700 dark:text-sky-300">
            <KeyRound size={15} /> JWT 续期与刷新
          </h3>
          <p className="text-xs leading-relaxed text-sky-800 dark:text-sky-200">
            JWT 默认 13 天过期。带有刷新令牌的账号（显示闪电图标）可点击「刷新」自动续期。
            不支持自动刷新的账号请点击「续期」图标，系统会启动代理并切换到该账号，通过代理捕获新 JWT。
          </p>
        </section>
      </div>
    </Modal>
  );
}
