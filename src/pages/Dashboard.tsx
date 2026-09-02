import { useMemo } from 'react';
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
import {
  ShieldAlert,
  ExternalLink,
  RefreshCw,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import SetupGuide from '../components/SetupGuide';
import { StatCard } from '../components/ui';
import { useAppStore } from '../store';
import { api } from '../lib/tauri';
import { useIsDark } from '../lib/useIsDark';

function formatUptime(startedAt: number | null): string | null {
  if (startedAt == null) return null;
  const secs = Math.floor(Date.now() / 1000 - startedAt);
  if (secs < 60) return `${secs}秒`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}分${secs % 60}秒`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}时${mins % 60}分`;
}

export default function Dashboard() {
  const accounts = useAppStore((s) => s.accounts);
  const env = useAppStore((s) => s.env);
  const proxy = useAppStore((s) => s.proxy);
  const apiStatus = useAppStore((s) => s.apiStatus);
  const certInstalled = useAppStore((s) => s.certInstalled);
  const toast = useAppStore((s) => s.pushToast);
  const isDark = useIsDark();

  const total = accounts.length;
  const checkedToday = accounts.filter((a) => a.checked_today).length;
  const totalCredits = useMemo(
    () => accounts.reduce((s, a) => s + (a.remaining_credits ?? 0), 0),
    [accounts],
  );
  const warned = accounts.filter(
    (a) => a.jwt_exp_hours !== null && a.jwt_exp_hours <= 24,
  ).length;

  const top = useMemo(
    () =>
      [...accounts]
        .filter((a) => a.remaining_credits != null && a.remaining_credits > 0)
        .sort((a, b) => (b.remaining_credits ?? 0) - (a.remaining_credits ?? 0))
        .slice(0, 10)
        .map((a) => ({ name: a.name, credits: a.remaining_credits as number })),
    [accounts],
  );

  const refresh = async () => {
    toast('info', '刷新中…');
    const s = useAppStore.getState();
    await Promise.all([
      s.refreshEnv(),
      s.refreshCert(),
      s.refreshProxy(),
      s.refreshApiStatus(),
      s.refreshAccounts(),
      s.refreshGroups(),
      s.refreshCreditsHistory(),
    ]);
    // 刷新剩余可用积分（会再次 refreshAccounts 更新 UI）
    void api.accounts.refreshRemainingCredits().then(() => s.refreshAccounts()).catch(() => {});
    toast('success', '已刷新');
  };

  const openTrae = async () => {
    await useAppStore.getState().openTraeWithProxy();
  };

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="概览"
        desc="Trae Work 多账号签到工作台 · 一眼掌握状态与快捷入口"
        actions={
          <button onClick={refresh} className="btn-outline">
            <RefreshCw size={15} /> 刷新
          </button>
        }
      />

      <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
        <StatCard label="账号总数" value={total} hint={`今日已签 ${checkedToday}`} tone="brand" />
        <StatCard label="积分总额" value={totalCredits.toLocaleString('zh-CN', { minimumFractionDigits: 0, maximumFractionDigits: 2 })} hint="总剩余可用积分" tone="amber" />
        <StatCard
          label="代理状态"
          value={proxy.running ? `运行 :${proxy.port}` : '未启动'}
          hint={proxy.running ? `已捕获 ${proxy.captured} 次请求 · 运行 ${formatUptime(proxy.started_at)}` : undefined}
          tone={proxy.running ? 'green' : 'slate'}
        />
        <StatCard
          label="API 服务"
          value={apiStatus?.running ? `运行 :${apiStatus.port}` : '未启动'}
          hint={apiStatus?.running ? `请求 ${apiStatus.total_requests} 次 · 运行 ${formatUptime(apiStatus.started_at)}` : undefined}
          tone={apiStatus?.running ? 'green' : 'slate'}
        />
        <StatCard
          label="JWT 告警"
          value={warned}
          hint="24h 内将过期"
          tone={warned > 0 ? 'red' : 'slate'}
        />
      </div>

      {!env?.installed && (
        <div className="mt-5 card flex items-center justify-between gap-4 p-4">
          <div className="flex items-center gap-3">
            <ShieldAlert className="text-amber-500" />
            <div>
              <div className="font-medium">未检测到 Trae Work 安装</div>
              <div className="text-xs text-slate-500">请先安装 Trae Work，再启动代理进行账号登录态捕获。</div>
            </div>
          </div>
          <button
            onClick={() => void openTrae()}
            className="btn-outline"
          >
            <ExternalLink size={15} /> {env?.installed ? '打开 Trae Work' : '前往下载'}
          </button>
        </div>
      )}
      {!certInstalled && env?.installed && (
        <div className="mt-3 card flex items-center justify-between gap-4 p-4">
          <div className="flex items-center gap-3">
            <ShieldAlert className="text-amber-500" />
            <div>
              <div className="font-medium">CA 证书尚未安装</div>
              <div className="text-xs text-slate-500">代理已启动但 TRAE 不信任代理证书将无法拦截签到接口。</div>
            </div>
          </div>
          <button
            onClick={async () => {
              try {
                await api.cert.install();
                await useAppStore.getState().refreshCert();
                toast('success', '证书安装成功');
              } catch (e) {
                toast('error', `证书安装失败：${String(e)}`);
              }
            }}
            className="btn-primary"
          >
            一键安装证书
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
            <span className="text-xs text-slate-400">按可用剩余积分排序</span>
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
                  formatter={(v: number) => [v.toLocaleString('zh-CN', { maximumFractionDigits: 2 }), '可用积分']}
                />
                <Bar dataKey="credits" radius={[8, 8, 0, 0]} maxBarSize={44}>
                  {top.map((_, i) => {
                    // Top 1-3 使用强调色，其余渐淡；暗色模式下反转明度
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
                    formatter={(v: number) => v >= 1000 ? `${(v / 1000).toFixed(1)}k` : v.toFixed(0)}
                    style={{ fontSize: 10, fill: isDark ? '#a1a1aa' : '#94a3b8', fontWeight: 500 }}
                  />
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}

      <div className="mt-5">
        <SetupGuide />
      </div>
    </div>
  );
}
