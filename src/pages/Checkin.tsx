import { useMemo, useState, useEffect } from 'react';
import { PlayCircle, CheckCircle2, XCircle, Clock, AlertCircle, HelpCircle, AlertTriangle, Snowflake } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, Progress } from '../components/ui';
import { useAppStore } from '../store';
import type { AccountView } from '../types';

function MiniJwtBadge({ hours }: { hours: number | null }) {
  if (hours === null) return <span className="text-xs text-slate-400"><HelpCircle size={11} className="inline" /> 未知</span>;
  if (hours <= 0) return <span className="text-xs text-rose-500"><XCircle size={11} className="inline" /> 过期</span>;
  if (hours <= 24) return <span className="text-xs text-amber-500"><AlertTriangle size={11} className="inline" /> {hours.toFixed(1)}h</span>;
  return <span className="text-xs text-emerald-500"><CheckCircle2 size={11} className="inline" /> {hours.toFixed(0)}h</span>;
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

function MiniCooldownBadge({ type, until }: { type: string; until: number | null }) {
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
    <span className={`text-xs ${isPermanent ? 'text-rose-500' : 'text-amber-500'}`}>
      <Snowflake size={11} className="inline" /> {label}{remaining && ` ${remaining}`}
    </span>
  );
}

export default function Checkin() {
  const accounts = useAppStore((s) => s.accounts);
  const groups = useAppStore((s) => s.groups);
  const settings = useAppStore((s) => s.settings);
  const checkin = useAppStore((s) => s.checkin);
  const startCheckin = useAppStore((s) => s.startCheckin);

  const [scope, setScope] = useState<'all' | 'group' | 'selected'>('all');
  const [groupId, setGroupId] = useState<string>(groups[0]?.id ?? '');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [skipChecked, setSkipChecked] = useState(true);
  const [skipExpired, setSkipExpired] = useState(true);

  useEffect(() => {
    if (settings) {
      setSkipChecked(settings.checkin_skip_checked);
      setSkipExpired(settings.checkin_skip_expired);
    }
  }, [settings]);

  // groups 加载后自动选中第一个分组
  useEffect(() => {
    if (groups.length > 0 && !groupId) setGroupId(groups[0].id);
  }, [groups, groupId]);

  // 根据范围过滤出候选账号列表
  const candidateAccounts = useMemo(() => {
    if (scope === 'all') return accounts;
    if (scope === 'group') return accounts.filter((a) => a.group_id === groupId);
    return accounts; // selected 模式下展示全部，通过 checkbox 勾选
  }, [scope, groupId, accounts]);

  const candidateIds = useMemo(() => {
    if (scope === 'selected') return [...selected];
    return candidateAccounts.map((a) => a.user_id);
  }, [scope, selected, candidateAccounts]);

  const toggle = (uid: string) => {
    setSelected((prev) => {
      const n = new Set(prev);
      if (n.has(uid)) n.delete(uid);
      else n.add(uid);
      return n;
    });
  };

  const toggleAll = () => {
    if (selected.size === accounts.length) {
      setSelected(new Set());
    } else {
      setSelected(new Set(accounts.map((a) => a.user_id)));
    }
  };

  const start = async () => {
    if (candidateIds.length === 0) return;
    let scopeArg: string = 'all';
    if (scope === 'group') scopeArg = `group:${groupId}`;
    if (scope === 'selected') scopeArg = 'selected';
    await startCheckin({
      scope: scopeArg,
      user_ids: scope === 'selected' ? candidateIds : undefined,
      skip_checked_in: skipChecked,
      skip_expired: skipExpired,
    });
  };

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="一键签到"
        desc="按账号范围与跳过规则发起批量签到，实时查看进度"
      />

      <div className="mb-5 flex items-start gap-3 rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm dark:border-amber-700 dark:bg-amber-900/20">
        <AlertTriangle size={18} className="mt-0.5 shrink-0 text-amber-500" />
        <div className="text-amber-700 dark:text-amber-300">
          <div className="font-medium">请勿一天内多次签到</div>
          <div className="mt-0.5 text-xs">每个账号每天只需签到一次，重复签到可能导致账号被官方禁封。建议开启「跳过今日已签」选项。</div>
        </div>
      </div>

      <div className="card mb-5 p-4">
        <div className="grid gap-3 md:grid-cols-2">
          <div>
            <label className="label">签到范围</label>
            <div className="flex flex-wrap gap-2">
              {([
                { k: 'all', t: '全部账号' },
                { k: 'group', t: '指定分组' },
                { k: 'selected', t: '手动勾选' },
              ] as const).map((o) => (
                <button
                  key={o.k}
                  onClick={() => setScope(o.k)}
                  className={`chip border ${scope === o.k ? 'border-brand-500 bg-brand-50 text-brand-700 dark:bg-brand-500/15 dark:text-brand-300' : 'border-slate-300 text-slate-500'}`}
                >
                  {o.t}
                </button>
              ))}
            </div>
          </div>
          {scope === 'group' && (
            <div>
              <label className="label">选择分组</label>
              <select className="input" value={groupId} onChange={(e) => setGroupId(e.target.value)}>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>{g.name} ({g.count})</option>
                ))}
              </select>
            </div>
          )}
        </div>

        {/* 账号列表 - 所有范围都展示 */}
        <div className="mt-4">
          <div className="mb-2 flex items-center justify-between">
            <label className="label">
              {scope === 'selected'
                ? `勾选账号（${selected.size}/${accounts.length}）`
                : `参与签到的账号（${candidateAccounts.length}）`}
            </label>
            {scope === 'selected' && (
              <button onClick={toggleAll} className="text-xs text-brand-500 hover:underline">
                {selected.size === accounts.length ? '取消全选' : '全选'}
              </button>
            )}
          </div>
          <div className="rounded-lg border border-slate-200 dark:border-zinc-700">
            <table className="w-full text-sm">
              <thead className="bg-slate-50 text-xs text-slate-500 dark:bg-zinc-900">
                <tr>
                  {scope === 'selected' && <th className="w-8 px-3 py-1.5"></th>}
                  <th className="px-3 py-1.5 text-left">账号</th>
                  <th className="px-3 py-1.5 text-left">JWT</th>
                  <th className="px-3 py-1.5 text-left">今日</th>
                  <th className="px-3 py-1.5 text-left">冷却</th>
                  <th className="px-3 py-1.5 text-right">签到积分</th>
                </tr>
              </thead>
              <tbody>
                {candidateAccounts.length === 0 ? (
                  <tr>
                    <td colSpan={scope === 'selected' ? 6 : 5} className="px-3 py-4 text-center text-xs text-slate-400">
                      {scope === 'group' ? '该分组下没有账号' : '暂无账号'}
                    </td>
                  </tr>
                ) : (
                  candidateAccounts.map((a) => {
                    const isCandidate = scope === 'selected' ? selected.has(a.user_id) : true;
                    return (
                      <tr
                        key={a.user_id}
                        className={`border-t border-slate-100 dark:border-zinc-800 ${scope === 'selected' && !isCandidate ? 'opacity-40' : ''}`}
                      >
                        {scope === 'selected' && (
                          <td className="px-3 py-1.5">
                            <input
                              type="checkbox"
                              checked={selected.has(a.user_id)}
                              onChange={() => toggle(a.user_id)}
                            />
                          </td>
                        )}
                        <td className="px-3 py-1.5">
                          <div className="font-medium">{a.name}</div>
                          <div className="text-xs text-slate-400">{a.user_id}</div>
                        </td>
                        <td className="px-3 py-1.5"><MiniJwtBadge hours={a.jwt_exp_hours} /></td>
                        <td className="px-3 py-1.5">
                          {a.checked_today ? (
                            <Badge tone="green">已签</Badge>
                          ) : (
                            <Badge tone="slate">未签</Badge>
                          )}
                        </td>
                        <td className="px-3 py-1.5">
                          {a.cooldown_type ? (
                            <MiniCooldownBadge type={a.cooldown_type} until={a.cooldown_until} />
                          ) : (
                            <span className="text-xs text-slate-300">-</span>
                          )}
                        </td>
                        <td className="px-3 py-1.5 text-right tabular-nums text-xs">
                          {a.credits != null ? a.credits.toLocaleString() : '-'}
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={skipChecked} onChange={(e) => setSkipChecked(e.target.checked)} />
            跳过今日已签
          </label>
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={skipExpired} onChange={(e) => setSkipExpired(e.target.checked)} />
            跳过 JWT 过期
          </label>
          <span className="ml-auto text-xs text-slate-500">
            目标：<b>{candidateIds.length}</b> 个账号
          </span>
        </div>
        <div className="mt-4 flex justify-end">
          <button
            onClick={start}
            disabled={checkin.active || candidateIds.length === 0}
            className="btn-primary"
          >
            <PlayCircle size={16} /> {checkin.active ? '签到进行中…' : '开始签到'}
          </button>
        </div>
      </div>

      {checkin.active || checkin.total > 0 ? (
        <div className="card p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="font-medium">实时进度</h3>
            {checkin.active ? (
              <Badge tone="blue">运行中</Badge>
            ) : checkin.done ? (
              <Badge tone={checkin.done.failed > 0 ? 'amber' : 'green'}>
                完成：成功 {checkin.done.ok}，已签 {checkin.done.already}，失败 {checkin.done.failed}
              </Badge>
            ) : null}
          </div>
          <div className="mb-3">
            <Progress value={checkin.index} max={checkin.total || 1} />
            <div className="mt-1 text-xs text-slate-500">
              {checkin.index}/{checkin.total}
            </div>
          </div>
          <div className="max-h-80 space-y-1 overflow-auto">
            {Array.from({ length: checkin.total }).map((_, i) => {
              const r = checkin.results[i];
              if (!r) {
                return (
                  <div key={i} className="flex items-center gap-2 rounded border border-slate-200 px-3 py-2 text-sm dark:border-zinc-700">
                    <Clock size={14} className="text-slate-400" />
                    <span className="text-slate-400">等待中…</span>
                  </div>
                );
              }
              const tone =
                r.status === 'success'
                  ? 'text-emerald-600 dark:text-emerald-300'
                  : r.status === 'already'
                  ? 'text-sky-600 dark:text-sky-300'
                  : r.status === 'fail'
                  ? 'text-rose-600 dark:text-rose-300'
                  : 'text-slate-500';
              const Icon =
                r.status === 'success'
                  ? CheckCircle2
                  : r.status === 'already'
                  ? CheckCircle2
                  : r.status === 'fail'
                  ? XCircle
                  : AlertCircle;
              return (
                <div key={i} className="flex items-center gap-2 rounded border border-slate-200 px-3 py-2 text-sm dark:border-zinc-700">
                  <Icon size={14} className={tone} />
                  <span className="w-8 text-right text-xs text-slate-400">{r.index}</span>
                  <span className="flex-1 truncate">{r.name}</span>
                  <span className={`text-xs ${tone}`}>
                    {r.status === 'success' && `积分+${r.delta ?? 0}`}
                    {r.status === 'already' && `积分+${r.credits ?? 0}`}
                    {r.status === 'fail' && (r.message ?? '失败')}
                  </span>
                  {r.error_type && (
                    <span className="text-xs text-amber-500">
                      <Snowflake size={11} className="inline" /> {COOLDOWN_LABELS[r.error_type] ?? r.error_type}
                    </span>
                  )}
                  {r.elapsed != null && <span className="text-xs text-slate-400">{r.elapsed.toFixed(1)}s</span>}
                </div>
              );
            })}
          </div>
        </div>
      ) : null}

      <div className="mt-5 text-xs text-slate-400">
        提示：如跳过规则默认开启，可在「设置」中调整。
        开关项遵循 settings.checkin_skip_checked 与 settings.checkin_skip_expired（{settings ? '已生效' : '加载中'}）。
      </div>
    </div>
  );
}
