// WorkBuddy 概览：对齐 Trae Dashboard 的结构（统计卡 + 告警卡 + 积分 Top 柱状图）
import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  ResponsiveContainer,
  Tooltip,
  CartesianGrid,
  Cell,
  LabelList,
} from 'recharts';
import { RefreshCw, ShieldAlert, Users } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { EmptyState, StatCard } from '../components/ui';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import { useIsDark } from '../lib/useIsDark';
import type { WorkBuddyAccountMeta, WorkBuddyCreditSummary } from '../types';
import { MS_PER_DAY, formatNum } from './wb-shared';

export default function WorkBuddyDashboard() {
  const toast = useAppStore((s) => s.pushToast);
  const setView = useAppStore((s) => s.setView);
  const isDark = useIsDark();

  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[] | null>(null);
  const [credits, setCredits] = useState<WorkBuddyCreditSummary[] | null>(null);
  const [loading, setLoading] = useState(false);

  const reload = useCallback(
    async (withToast: boolean) => {
      setLoading(true);
      try {
        const [acc, cr] = await Promise.all([
          api.workbuddy.listAccounts(),
          api.workbuddy.credits().catch(() => null as WorkBuddyCreditSummary[] | null),
        ]);
        setAccounts(acc);
        setCredits(cr);
        if (withToast) toast('success', '已刷新');
      } catch (e) {
        toast('error', `读取 WorkBuddy 数据失败：${String(e)}`);
      } finally {
        setLoading(false);
      }
    },
    [toast],
  );

  useEffect(() => {
    void reload(false);
  }, [reload]);

  const total = accounts?.length ?? 0;
  const checkedToday = accounts?.filter((a) => a.checkedToday).length ?? 0;

  const totalRemaining = useMemo(
    () =>
      (credits ?? [])
        .filter((c) => c.ok)
        .reduce((s, c) => s + (c.totalRemaining ?? 0), 0),
    [credits],
  );

  // Token 告警：需重登或 24h 内临期/已过期
  const warned = useMemo(
    () =>
      (accounts ?? []).filter(
        (a) =>
          a.needsRelogin ||
          (a.expiresAt != null && a.expiresAt - Date.now() < MS_PER_DAY),
      ).length,
    [accounts],
  );

  const reloginAccounts = useMemo(
    () => (accounts ?? []).filter((a) => a.needsRelogin),
    [accounts],
  );

  // 积分 Top 榜：查询成功且剩余 > 0 的账号，按剩余额度排序取前 10
  const top = useMemo(
    () =>
      (credits ?? [])
        .filter((c) => c.ok && (c.totalRemaining ?? 0) > 0)
        .sort((a, b) => (b.totalRemaining ?? 0) - (a.totalRemaining ?? 0))
        .slice(0, 10)
        .map((c) => ({ name: c.accountName, credits: c.totalRemaining as number })),
    [credits],
  );

  if (accounts != null && accounts.length === 0) {
    return (
      <div className="animate-fade-in">
        <PageHeader
          title="概览"
          desc="WorkBuddy 多账号签到工作台 · 一眼掌握状态与快捷入口"
          actions={
            <button onClick={() => void reload(true)} className="btn-outline">
              <RefreshCw size={15} /> 刷新
            </button>
          }
        />
        <EmptyState
          icon={<Users size={28} />}
          title="还没有 WorkBuddy 账号"
          hint="先到「账号管理」导入本机已登录的 WorkBuddy 客户端账号，或手动粘贴 access_token 添加。"
        />
      </div>
    );
  }

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="概览"
        desc="WorkBuddy 多账号签到工作台 · 一眼掌握状态与快捷入口"
        actions={
          <button onClick={() => void reload(true)} className="btn-outline">
            <RefreshCw size={15} className={loading ? 'animate-spin' : undefined} /> 刷新
          </button>
        }
      />

      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatCard
          label="账号总数"
          value={accounts == null ? '-' : total}
          hint={`今日已签 ${checkedToday}`}
          tone="brand"
        />
        <StatCard
          label="积分总额"
          value={credits == null ? '-' : formatNum(totalRemaining)}
          hint="总剩余可用额度"
          tone="amber"
        />
        <StatCard
          label="今日已签"
          value={accounts == null ? '-' : `${checkedToday}/${total}`}
          hint={total - checkedToday > 0 ? `还有 ${total - checkedToday} 个未签` : '全部完成'}
          tone={total > 0 && checkedToday === total ? 'green' : 'slate'}
        />
        <StatCard
          label="Token 告警"
          value={accounts == null ? '-' : warned}
          hint="需重登或 24h 内临期"
          tone={warned > 0 ? 'red' : 'slate'}
        />
      </div>

      {reloginAccounts.length > 0 && (
        <div className="mt-5 card flex items-center justify-between gap-4 p-4">
          <div className="flex items-center gap-3">
            <ShieldAlert className="text-amber-500" />
            <div>
              <div className="font-medium">{reloginAccounts.length} 个账号需要重新登录</div>
              <div className="max-w-md truncate text-xs text-slate-500">
                {reloginAccounts.map((a) => a.nickname || a.email || a.uid).join('、')}
                {reloginAccounts.some((a) => a.needsReloginReason)
                  ? `（${reloginAccounts[0].needsReloginReason ?? 'refresh token 已失效'}）`
                  : ''}
              </div>
            </div>
          </div>
          <button onClick={() => setView('wb-accounts')} className="btn-outline">
            前往账号管理
          </button>
        </div>
      )}

      {top.length > 0 && (
        <div className="mt-5 card p-5">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <h3 className="font-medium">积分榜 Top 榜</h3>
              <span className="rounded-full bg-zinc-100 px-2 py-0.5 text-xs font-medium text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
                Top {top.length}
              </span>
            </div>
            <span className="text-xs text-slate-400">按可用剩余额度排序</span>
          </div>
          <div className="h-72">
            <ResponsiveContainer>
              <BarChart data={top} margin={{ top: 24, right: 16, left: 0, bottom: 4 }} barCategoryGap="36%">
                <CartesianGrid strokeDasharray="3 3" stroke={isDark ? '#3f3f46' : '#e2e8f0'} opacity={0.25} vertical={false} />
                <XAxis
                  dataKey="name"
                  tick={{ fontSize: 11, fill: isDark ? '#a1a1aa' : '#94a3b8' }}
                  interval={0}
                  angle={-20}
                  textAnchor="end"
                  height={52}
                  axisLine={{ stroke: isDark ? '#3f3f46' : '#e2e8f0' }}
                  tickLine={false}
                />
                <YAxis tick={{ fontSize: 11, fill: isDark ? '#a1a1aa' : '#94a3b8' }} axisLine={false} tickLine={false} width={48} />
                <Tooltip
                  cursor={{ fill: isDark ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.03)' }}
                  contentStyle={{
                    fontSize: 12,
                    borderRadius: 10,
                    border: `1px solid ${isDark ? '#3f3f46' : '#e2e8f0'}`,
                    background: isDark ? '#18181b' : '#fff',
                    color: isDark ? '#e4e4e7' : '#1e293b',
                    boxShadow: '0 6px 16px rgba(0,0,0,0.1)',
                    padding: '8px 12px',
                  }}
                  formatter={(v: number) => [formatNum(v), '可用额度']}
                />
                <Bar dataKey="credits" radius={[8, 8, 0, 0]} maxBarSize={44}>
                  {top.map((_, i) => {
                    // Top 1-3 使用强调色，其余渐淡；暗色模式下反转明度（与 Trae 概览一致）
                    const colors = isDark
                      ? ['#fafafa', '#e4e4e7', '#d4d4d8']
                      : ['#27272a', '#3f3f46', '#52525b'];
                    const fill = i < 3 ? colors[i] : isDark
                      ? `rgba(212,212,216,${Math.max(0.35, 0.6 - (i - 3) * 0.05).toFixed(2)})`
                      : `rgba(82,82,91,${Math.max(0.35, 0.6 - (i - 3) * 0.05).toFixed(2)})`;
                    return <Cell key={i} fill={fill} />;
                  })}
                  <LabelList
                    dataKey="credits"
                    position="top"
                    formatter={(v: number) => (v >= 1000 ? `${(v / 1000).toFixed(1)}k` : v.toFixed(0))}
                    style={{ fontSize: 10, fill: isDark ? '#a1a1aa' : '#94a3b8', fontWeight: 500 }}
                  />
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}
    </div>
  );
}
