import { useMemo, useState, useCallback } from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  ResponsiveContainer,
  Tooltip,
  CartesianGrid,
} from 'recharts';
import { Coins, RefreshCw } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { StatCard, Badge, EmptyState } from '../components/ui';
import { useAppStore } from '../store';
import { useIsDark } from '../lib/useIsDark';

function localDate(d: Date): string {
  const y = d.getFullYear();
  const m = `${d.getMonth() + 1}`.padStart(2, '0');
  const day = `${d.getDate()}`.padStart(2, '0');
  return `${y}-${m}-${day}`;
}

export default function Credits() {
  const accounts = useAppStore((s) => s.accounts);
  const groups = useAppStore((s) => s.groups);
  const creditsHistory = useAppStore((s) => s.creditsHistory);

  const creditsDaily = useAppStore((s) => s.creditsDaily);
  const isDark = useIsDark();
  const refreshRemainingCredits = useAppStore((s) => s.refreshRemainingCredits);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const refreshCreditsDaily = useAppStore((s) => s.refreshCreditsDaily);
  const refreshCreditsHistory = useAppStore((s) => s.refreshCreditsHistory);
  const pushToast = useAppStore((s) => s.pushToast);
  const [refreshing, setRefreshing] = useState(false);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      // 1. 刷新所有账号剩余积分（后端会更新 credits_daily.json 快照）
      await refreshRemainingCredits();
      // 2. 重新加载账号列表（remaining_credits 字段）
      await refreshAccounts();
      // 3. 重新加载每日积分快照
      await refreshCreditsDaily();
      // 4. 重新加载签到历史
      await refreshCreditsHistory();
      pushToast('success', '积分数据已刷新');
    } catch (err) {
      pushToast('error', `刷新失败：${String(err)}`);
    } finally {
      setRefreshing(false);
    }
  }, [refreshRemainingCredits, refreshAccounts, refreshCreditsDaily, refreshCreditsHistory, pushToast]);

  const rows = useMemo(
    () =>
      [...accounts].sort((a, b) => {
        const va = a.remaining_credits;
        const vb = b.remaining_credits;
        // null 排到最后
        if (va == null && vb == null) return 0;
        if (va == null) return 1;
        if (vb == null) return -1;
        return vb - va; // 降序
      }),
    [accounts],
  );
  const total = rows.reduce((s, a) => s + (a.remaining_credits ?? 0), 0);
  const avg = rows.length === 0 ? 0 : Math.round(total / rows.length);

  // 今日新增积分：优先使用 daily snapshot 的 earned 字段（含签到+购买）
  // 回退：仅签到 history delta
  const today = localDate(new Date());
  const todayNew = useMemo(() => {
    // 1. 优先从每日快照获取 earned（包含签到 + 非签到获得）
    const snap = creditsDaily.find((s) => s.date === today);
    if (snap && snap.earned > 0) return Math.round(snap.earned);
    // 2. 回退到签到 history delta
    const histVal = creditsHistory
      .filter((r) => r.date === today)
      .reduce((s, r) => s + (r.delta || 0), 0);
    if (histVal > 0) return histVal;
    // 3. 无任何数据时不显示
    return 0;
  }, [creditsDaily, creditsHistory, today]);

  const todayConsumed = useMemo(() => {
    const snap = creditsDaily.find((s) => s.date === today);
    return snap ? Math.round(snap.consumed) : 0;
  }, [creditsDaily, today]);

  // 近 7 日趋势：从 creditsDaily 快照取数据，补齐无数据的日期
  const trend = useMemo(() => {
    const map = new Map<string, { total: number; earned: number; consumed: number }>();
    for (const s of creditsDaily) {
      map.set(s.date, { total: s.total, earned: s.earned, consumed: s.consumed });
    }
    const days: { label: string; total: number; earned: number; consumed: number }[] = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(Date.now() - i * 86400000);
      const key = localDate(d);
      const snap = map.get(key);
      days.push({
        label: `${d.getMonth() + 1}/${d.getDate()}`,
        total: snap?.total ?? 0,
        earned: snap?.earned ?? 0,
        consumed: snap?.consumed ?? 0,
      });
    }
    // 如果今天没有快照，用当前 total 填充
    if (days.length > 0 && days[days.length - 1].total === 0 && total > 0) {
      days[days.length - 1].total = Math.round(total);
    }
    return days;
  }, [creditsDaily, total]);

  const hasTrend = trend.some((d) => d.total > 0 || d.earned > 0 || d.consumed > 0);

  return (
    <div className="animate-fade-in">
      <div className="flex items-center justify-between">
        <PageHeader
          title="积分看板"
          desc="查看每个账号的积分余额与趋势"
        />
        <button
          className="btn-ghost flex items-center gap-1.5 text-sm"
          onClick={handleRefresh}
          disabled={refreshing}
          title={refreshing ? '刷新中…' : '刷新数据'}
        >
          <RefreshCw size={15} className={refreshing ? 'animate-spin' : ''} />
          {refreshing ? '刷新中' : '刷新'}
        </button>
      </div>

      <div className="mb-5 grid grid-cols-2 gap-3 md:grid-cols-5">
        <StatCard label="可用积分总额" value={total.toLocaleString('zh-CN', { minimumFractionDigits: 0, maximumFractionDigits: 2 })} hint="总剩余可用积分" tone="violet" />
        <StatCard label="账号数" value={rows.length} tone="brand" />
        <StatCard label="平均可用积分" value={avg.toLocaleString()} tone="blue" />
        <StatCard label="今日新增积分" value={todayNew.toLocaleString()} tone="green" hint={today} />
        <StatCard label="今日消耗积分" value={todayConsumed.toLocaleString()} tone="amber" hint={today} />
      </div>

      <div className="card p-5">
        <div className="mb-4 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="font-medium">近 7 日积分趋势</h3>
            <span className="rounded-full bg-zinc-100 px-2 py-0.5 text-xs font-medium text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
              7 Days
            </span>
          </div>
          <div className="flex items-center gap-3 text-xs text-slate-400">
            <span className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded-full" style={{ background: '#6366f1' }} />
              积分总数
            </span>
            <span className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded-full" style={{ background: '#22c55e' }} />
              获得积分
            </span>
            <span className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded-full" style={{ background: '#f59e0b' }} />
              消耗积分
            </span>
          </div>
        </div>
        {accounts.length === 0 ? (
          <EmptyState icon={<Coins size={28} />} title="尚无账号数据" hint="添加账号后这里会展示积分趋势。" />
        ) : !hasTrend ? (
          <EmptyState
            icon={<Coins size={28} />}
            title="暂无趋势数据"
            hint="执行签到或刷新积分后，这里会展示每日积分变化趋势。"
          />
        ) : (
          <div className="h-56">
            <ResponsiveContainer>
              <LineChart data={trend} margin={{ top: 24, right: 16, left: 0, bottom: 4 }}>
                <CartesianGrid strokeDasharray="3 3" stroke={isDark ? '#3f3f46' : '#e2e8f0'} opacity={0.25} vertical={false} />
                <XAxis
                  dataKey="label"
                  tick={{ fontSize: 11, fill: isDark ? '#a1a1aa' : '#94a3b8' }}
                  axisLine={{ stroke: isDark ? '#3f3f46' : '#e2e8f0' }}
                  tickLine={false}
                />
                <YAxis tick={{ fontSize: 11, fill: isDark ? '#a1a1aa' : '#94a3b8' }} axisLine={false} tickLine={false} width={56} />
                <Tooltip
                  cursor={{ stroke: isDark ? '#52525b' : '#cbd5e1', strokeWidth: 1, strokeDasharray: '3 3' }}
                  contentStyle={{
                    fontSize: 12,
                    borderRadius: 10,
                    border: `1px solid ${isDark ? '#3f3f46' : '#e2e8f0'}`,
                    background: isDark ? '#18181b' : '#fff',
                    color: isDark ? '#e4e4e7' : '#1e293b',
                    boxShadow: '0 6px 16px rgba(0,0,0,0.1)',
                    padding: '8px 12px',
                  }}
                  formatter={(v: number, name: string) => {
                    const labels: Record<string, string> = { total: '积分总数', earned: '获得积分', consumed: '消耗积分' };
                    return [v.toLocaleString('zh-CN', { maximumFractionDigits: 2 }), labels[name] ?? name];
                  }}
                />
                <Line type="monotone" dataKey="total" stroke="#6366f1" strokeWidth={2.5} dot={{ r: 3, fill: '#6366f1', strokeWidth: 0 }} activeDot={{ r: 5 }} />
                <Line type="monotone" dataKey="earned" stroke="#22c55e" strokeWidth={2} dot={{ r: 3, fill: '#22c55e', strokeWidth: 0 }} activeDot={{ r: 5 }} />
                <Line type="monotone" dataKey="consumed" stroke="#f59e0b" strokeWidth={2} dot={{ r: 3, fill: '#f59e0b', strokeWidth: 0 }} activeDot={{ r: 5 }} />
              </LineChart>
            </ResponsiveContainer>
          </div>
        )}
      </div>

      <div className="mt-5 card overflow-hidden">
        <div className="border-b border-slate-100 px-5 py-3 dark:border-zinc-800">
          <h3 className="font-medium">账号积分明细</h3>
        </div>
        <table className="w-full text-sm">
          <thead className="bg-slate-50 text-xs uppercase text-slate-500 dark:bg-zinc-900">
            <tr>
              <th className="px-4 py-2 text-left">排名</th>
              <th className="px-4 py-2 text-left">账号</th>
              <th className="px-4 py-2 text-left">分组</th>
              <th className="px-4 py-2 text-right">剩余可用积分</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((a, i) => {
              const g = groups.find((x) => x.id === a.group_id);
              return (
                <tr key={a.user_id} className="border-t border-slate-200 dark:border-zinc-800">
                  <td className="px-4 py-2">#{i + 1}</td>
                  <td className="px-4 py-2">
                    <div className="font-medium">{a.name}</div>
                    <div className="text-xs text-slate-400">{a.user_id}</div>
                  </td>
                  <td className="px-4 py-2">
                    {g ? (
                      <Badge tone="slate">
                        <span className="inline-block h-2 w-2 rounded-full" style={{ background: g.color }} />
                        {g.name}
                      </Badge>
                    ) : (
                      <span className="text-xs text-slate-400">未分组</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums">{(a.remaining_credits ?? 0).toLocaleString('zh-CN', { minimumFractionDigits: 0, maximumFractionDigits: 2 })}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
