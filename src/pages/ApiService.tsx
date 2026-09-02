import { useEffect, useState, useCallback } from 'react';
import {
  Play,
  Square,
  Save,
  RefreshCw,
  CheckCircle2,
  XCircle,
  Activity,
  Globe,
  Eye,
  EyeOff,
  Eraser,
  Copy,
  Info,
} from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, StatCard } from '../components/ui';
import { useAppStore } from '../store';
import { api } from '../lib/tauri';
import { withMinDelay } from '../lib/delay';
import type { Settings, ApiServiceStatus, PoolStatus } from '../types';

/** 将 API Key 打码：保留前4后4，中间用 **** 替代 */
function maskApiKey(key: string): string {
  if (!key) return '';
  if (key.length <= 8) return '****';
  return `${key.slice(0, 4)}****${key.slice(-4)}`;
}

const MODEL_OPTIONS = [
  'glm-5.2',
  'glm-5.3',
  'glm-5-turbo',
  'glm-5',
  'deepseek-v4-flash',
  'deepseek-v4-pro',
  'kimi-k2.7-code',
  'kimi-k3',
  'doubao-seed-2.1-pro',
  'doubao-seed-2.1-turbo',
  'doubao-seed-2.0-code',
  'minimax-m3',
  'qwen-3.7-plus',
];

export default function ApiService() {
  const settings = useAppStore((s) => s.settings);
  const saveSettings = useAppStore((s) => s.saveSettings);
  const refreshSettings = useAppStore((s) => s.refreshSettings);
  const accounts = useAppStore((s) => s.accounts);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const toast = useAppStore((s) => s.pushToast);

  const [form, setForm] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<ApiServiceStatus | null>(null);
  const [poolStatus, setPoolStatus] = useState<PoolStatus[]>([]);
  const [enabledUids, setEnabledUids] = useState<Set<string>>(new Set());
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [savingPool, setSavingPool] = useState(false);
  const [clearingCooldowns, setClearingCooldowns] = useState(false);
  const [refreshingPool, setRefreshingPool] = useState(false);
  const [copying, setCopying] = useState(false);

  useEffect(() => {
    void refreshSettings();
    void refreshAccounts();
    void loadPool();
    void refreshStatus();
  }, [refreshSettings, refreshAccounts]);

  useEffect(() => {
    if (settings && !form) {
      setForm(settings);
    }
  }, [settings, form]);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await api.apiServer.status();
      setStatus(s);
      if (s.running) {
        try {
          const ps = await api.apiServer.poolStatus();
          setPoolStatus(ps);
        } catch {
          /* ignore */
        }
      } else {
        setPoolStatus([]);
      }
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    if (!status?.running) return;
    const id = setInterval(() => void refreshStatus(), 3000);
    return () => clearInterval(id);
  }, [status?.running, refreshStatus]);

  const loadPool = async () => {
    setRefreshingPool(true);
    try {
      const pool = await withMinDelay(api.apiServer.poolList());
      setEnabledUids(new Set(pool.enabled_uids));
    } catch {
      /* ignore */
    } finally {
      setRefreshingPool(false);
    }
  };

  const update = <K extends keyof Settings>(key: K, val: Settings[K]) => {
    setForm((prev) => (prev ? { ...prev, [key]: val } : prev));
  };

  const save = async () => {
    if (!form) return;
    setSaving(true);
    try {
      await withMinDelay(saveSettings(form));
      toast('success', '配置已保存');
    } catch {
      /* toast 已发出 */
    } finally {
      setSaving(false);
    }
  };

  const start = async () => {
    setStarting(true);
    try {
      // 启动前自动保存当前勾选的账号池，避免用户忘记点"保存"
      await api.apiServer.poolSet([...enabledUids]);
      const s = await withMinDelay(api.apiServer.start());
      setStatus(s);
      useAppStore.setState({ apiStatus: s });
      toast('success', `API 服务已启动（端口 ${s.port}，池内 ${enabledUids.size} 个账号）`);
      void refreshStatus();
    } catch (err) {
      toast('error', `启动失败：${String(err)}`);
    } finally {
      setStarting(false);
    }
  };

  const stop = async () => {
    setStopping(true);
    try {
      await withMinDelay(api.apiServer.stop());
      toast('info', 'API 服务已停止');
      setStatus(null);
      setPoolStatus([]);
      useAppStore.setState({ apiStatus: null });
    } catch (err) {
      toast('error', `停止失败：${String(err)}`);
    } finally {
      setStopping(false);
    }
  };

  const toggleUid = (uid: string) => {
    setEnabledUids((prev) => {
      const next = new Set(prev);
      if (next.has(uid)) next.delete(uid);
      else next.add(uid);
      return next;
    });
  };

  const savePool = async () => {
    setSavingPool(true);
    try {
      await withMinDelay(api.apiServer.poolSet([...enabledUids]));
      toast('success', '账号池已更新');
      if (status?.running) {
        toast('info', '需重启 API 服务以应用变更');
      }
    } catch (err) {
      toast('error', `保存账号池失败：${String(err)}`);
    } finally {
      setSavingPool(false);
    }
  };

  const clearAllCooldowns = async () => {
    setClearingCooldowns(true);
    try {
      const cleared = await withMinDelay(api.accounts.cooldownClearAll());
      if (cleared > 0) {
        toast('success', `已清除 ${cleared} 个账号的冷却状态`);
        void refreshStatus();
      } else {
        toast('info', '当前无冷却中的账号');
      }
    } catch (err) {
      toast('error', `清除冷却失败：${String(err)}`);
    } finally {
      setClearingCooldowns(false);
    }
  };

  const copyConfigExample = async () => {
    setCopying(true);
    const port = form?.api_port ?? 7864;
    const apiKey = form?.api_key ?? '';
    const maskedKey = apiKey ? maskApiKey(apiKey) : '';
    const model = form?.api_default_model ?? 'glm-5.2';
    const example = `# 客户端配置示例（OpenAI 兼容格式）
接口地址: http://127.0.0.1:${port}/v1
API Key:  ${maskedKey || '（留空则不鉴权）'}
模型 ID:  ${model}

# cURL 测试（请将 API Key 替换为完整值）
curl -X POST http://127.0.0.1:${port}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${maskedKey || 'your-api-key'}" \\
  -d '{
    "model": "${model}",
    "messages": [{"role": "user", "content": "你好"}],
    "stream": true
  }'`;
    try {
      await withMinDelay(navigator.clipboard.writeText(example));
      toast('success', '配置示例已复制到剪贴板');
    } catch {
      toast('error', '复制失败');
    } finally {
      setCopying(false);
    }
  };

  const running = status?.running ?? false;
  const poolCount = enabledUids.size;

  return (
    <div>
      <PageHeader
        title="API 服务"
        desc="OpenAI 兼容接口，通过账号池轮转实现多账号负载均衡（消耗 IDE 积分）"
        actions={
          running ? (
            <button
              className="btn-danger flex items-center gap-2"
              onClick={stop}
              disabled={stopping}
            >
              <Square size={16} />
              {stopping ? '停止中…' : '停止服务'}
            </button>
          ) : (
            <button
              className="btn-primary flex items-center gap-2"
              onClick={start}
              disabled={starting || poolCount === 0}
            >
              <Play size={16} />
              {starting ? '启动中…' : poolCount === 0 ? '请先选择账号' : '启动服务'}
            </button>
          )
        }
      />

      {/* 状态卡片 */}
      <div className="mb-5 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCard
          label="运行状态"
          value={running ? '运行中' : '已停止'}
          tone={running ? 'green' : 'slate'}
          hint={running ? `127.0.0.1:${status?.port ?? 0}` : '未启动'}
        />
        <StatCard
          label="总请求数"
          value={status?.total_requests ?? 0}
          tone="brand"
          hint="累计处理的 API 调用"
        />
        <StatCard
          label="活跃账号"
          value={status?.active_uid ? status.active_uid.slice(0, 8) + '…' : '—'}
          tone="blue"
          hint={status?.active_uid ? '当前正在处理请求' : '空闲'}
        />
        <StatCard
          label="池内账号"
          value={poolCount}
          tone="violet"
          hint="已选入轮转池的账号数"
        />
      </div>

      {status?.last_error && (
        <div className="mb-5 flex items-center gap-2 rounded-lg border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-800 dark:bg-rose-900/20 dark:text-rose-300">
          <XCircle size={16} className="shrink-0" />
          <span className="truncate">{status.last_error}</span>
        </div>
      )}

      {/* 积分类型说明 — 紧凑面板 */}
      <div className="mb-5 rounded-xl border border-amber-300/70 bg-amber-50/80 px-3.5 py-2.5 dark:border-amber-700/40 dark:bg-amber-900/10">
        <div className="mb-2 flex items-center gap-2">
          <Info size={14} className="shrink-0 text-amber-500 dark:text-amber-400" />
          <span className="text-xs font-semibold text-amber-800 dark:text-amber-200">积分体系说明</span>
          <span className="text-[11px] text-amber-600/60 dark:text-amber-400/40">本服务仅消耗 IDE 积分</span>
        </div>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {/* IDE 积分 */}
          <div className="rounded-lg border border-amber-300/60 bg-white/60 px-3 py-1.5 dark:border-amber-700/30 dark:bg-amber-900/5">
            <div className="mb-1 flex items-center justify-between">
              <span className="text-[11px] font-bold text-amber-800 dark:text-amber-200">IDE 积分（Trae CN）</span>
              <span className="rounded bg-amber-200 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:bg-amber-700/50 dark:text-amber-100">本服务使用</span>
            </div>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-slate-500 dark:text-zinc-400">
              <span className="font-mono text-amber-700 dark:text-amber-300">product_id 208</span>
              <span className="text-slate-300 dark:text-zinc-600">·</span>
              <span className="font-mono text-amber-700 dark:text-amber-300">llm_utils_chat</span>
              <span className="text-slate-300 dark:text-zinc-600">·</span>
              <span>IDE 套餐</span>
              <span className="text-slate-300 dark:text-zinc-600">·</span>
              <span>明文 JSON</span>
            </div>
          </div>
          {/* Work 积分 */}
          <div className="rounded-lg border border-slate-200 bg-slate-50/60 px-3 py-1.5 dark:border-zinc-700/50 dark:bg-zinc-800/30">
            <div className="mb-1 flex items-center justify-between">
              <span className="text-[11px] font-bold text-slate-500 dark:text-zinc-400">Work 积分（Trae Work CN）</span>
              <span className="rounded bg-slate-200 px-1.5 py-0.5 text-[10px] font-medium text-slate-500 dark:bg-zinc-700 dark:text-zinc-400">未采用</span>
            </div>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-slate-500 dark:text-zinc-400">
              <span className="font-mono text-slate-500 dark:text-zinc-400">product_id 209</span>
              <span className="text-slate-300 dark:text-zinc-600">·</span>
              <span className="font-mono text-slate-500 dark:text-zinc-400">create_agent_task</span>
              <span className="text-slate-300 dark:text-zinc-600">·</span>
              <span>签到/购买</span>
              <span className="text-slate-300 dark:text-zinc-600">·</span>
              <span>需 TTNet 加密</span>
            </div>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 items-stretch gap-5 lg:grid-cols-2">
        {/* 配置卡片 */}
        <div className="card flex flex-col p-5">
          <div className="mb-4 flex items-center gap-2">
            <Globe size={18} className="text-brand-500" />
            <h2 className="text-sm font-semibold text-slate-800 dark:text-zinc-100">接口配置</h2>
          </div>

          <div className="flex-1 space-y-4">
            <div>
              <label className="mb-1 block text-xs font-medium text-slate-500 dark:text-zinc-400">
                监听端口
              </label>
              <input
                type="number"
                className="input"
                value={form?.api_port ?? 7864}
                onChange={(e) => update('api_port', parseInt(e.target.value) || 7864)}
                disabled={running}
              />
              <p className="mt-1 text-xs text-slate-400">服务运行时无法修改</p>
            </div>

            <div>
              <label className="mb-1 block text-xs font-medium text-slate-500 dark:text-zinc-400">
                API Key（留空则不鉴权）
              </label>
              <div className="relative">
                <input
                  type={showApiKey ? 'text' : 'password'}
                  className="input pr-10"
                  placeholder="sk-..."
                  value={form?.api_key ?? ''}
                  onChange={(e) => update('api_key', e.target.value)}
                  disabled={running}
                />
                <button
                  type="button"
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600 dark:text-zinc-500 dark:hover:text-zinc-300"
                  onClick={() => setShowApiKey((v) => !v)}
                  tabIndex={-1}
                >
                  {showApiKey ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
              <p className="mt-1 text-xs text-slate-400">
                客户端请求需携带 Authorization: Bearer &lt;key&gt;
              </p>
            </div>

            <div>
              <label className="mb-1 block text-xs font-medium text-slate-500 dark:text-zinc-400">
                默认模型
              </label>
              <select
                className="input"
                value={form?.api_default_model ?? 'glm-5.2'}
                onChange={(e) => update('api_default_model', e.target.value)}
                disabled={running}
              >
                {MODEL_OPTIONS.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
              <p className="mt-1 text-xs text-slate-400">
                上游接口：llm_utils_chat（IDE 积分，product_id 208）
              </p>
            </div>

            <div className="rounded-lg bg-slate-50 p-3 text-xs text-slate-500 dark:bg-zinc-800/50 dark:text-zinc-400">
              <div className="mb-2 flex items-center justify-between">
                <p className="font-medium">使用方式 & 配置示例</p>
                <span className="rounded bg-blue-100 px-1.5 py-0.5 text-[10px] text-blue-600 dark:bg-blue-900/30 dark:text-blue-400">
                  IDE 积分
                </span>
                <button
                  className="btn-ghost flex items-center gap-1 !p-1 text-xs"
                  onClick={copyConfigExample}
                  disabled={copying}
                  title="复制完整配置示例"
                >
                  <Copy size={12} className={copying ? 'animate-pulse' : ''} />
                  {copying ? '复制中…' : '复制示例'}
                </button>
              </div>
              <div className="space-y-1.5">
                <div>
                  <span className="text-slate-400">接口地址：</span>
                  <code className="break-all text-[11px]">
                    http://127.0.0.1:{form?.api_port ?? 7864}/v1
                  </code>
                </div>
                <div>
                  <span className="text-slate-400">API Key：</span>
                  <code className="text-[11px]">
                    {form?.api_key ? maskApiKey(form.api_key) : '（留空则不鉴权）'}
                  </code>
                </div>
                <div>
                  <span className="text-slate-400">模型 ID：</span>
                  <code className="text-[11px]">{form?.api_default_model ?? 'glm-5.2'}</code>
                </div>
                <div className="pt-1">
                  <span className="text-slate-400">其他端点：</span>
                </div>
                <code className="block break-all text-[11px]">
                  GET http://127.0.0.1:{form?.api_port ?? 7864}/v1/models
                </code>
                <code className="block break-all text-[11px]">
                  GET http://127.0.0.1:{form?.api_port ?? 7864}/health
                </code>
              </div>
            </div>

            <button
              className="btn-secondary flex items-center gap-2"
              onClick={save}
              disabled={saving || running}
            >
              <Save size={15} />
              {saving ? '保存中…' : '保存配置'}
            </button>
          </div>
        </div>

        {/* 账号池卡片 */}
        <div className="card flex flex-col p-5">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Activity size={18} className="text-brand-500" />
              <h2 className="text-sm font-semibold text-slate-800 dark:text-zinc-100">
                账号池选择
              </h2>
            </div>
            <button
              className="btn-ghost flex items-center gap-1 text-xs"
              onClick={() => void loadPool()}
              disabled={refreshingPool}
            >
              <RefreshCw size={13} className={refreshingPool ? 'animate-spin' : ''} />
              {refreshingPool ? '刷新中…' : '刷新'}
            </button>
          </div>

          {accounts.length === 0 ? (
            <p className="flex-1 py-8 text-center text-sm text-slate-400">暂无账号，请先在账号管理中添加</p>
          ) : (
            <>
              <div className="mb-3 flex items-center gap-2">
                <button
                  className="text-xs text-brand-600 hover:underline dark:text-brand-400"
                  onClick={() =>
                    setEnabledUids(new Set(accounts.map((a) => a.user_id)))
                  }
                >
                  全选
                </button>
                <span className="text-slate-300">|</span>
                <button
                  className="text-xs text-brand-600 hover:underline dark:text-brand-400"
                  onClick={() => setEnabledUids(new Set())}
                >
                  清空
                </button>
                <span className="ml-auto text-xs text-slate-400">
                  已选 {enabledUids.size} / {accounts.length}
                </span>
              </div>

              <div className="flex-1 space-y-1">
                {accounts.map((a) => {
                  const checked = enabledUids.has(a.user_id);
                  const poolItem = poolStatus.find((p) => p.uid === a.user_id);
                  return (
                    <label
                      key={a.user_id}
                      className="flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 transition hover:bg-slate-50 dark:hover:bg-zinc-800/50"
                    >
                      <input
                        type="checkbox"
                        className="h-4 w-4 rounded border-slate-300 text-brand-600 focus:ring-brand-500"
                        checked={checked}
                        onChange={() => toggleUid(a.user_id)}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-medium text-slate-700 dark:text-zinc-200">
                          {a.name}
                        </div>
                        <div className="truncate text-xs text-slate-400">
                          {a.user_id}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        {a.remaining_credits != null && (
                          <span className="text-xs tabular-nums text-slate-500 dark:text-zinc-400">
                            {a.remaining_credits.toFixed(0)} 积分
                          </span>
                        )}
                        {poolItem?.cooling && (
                          <Badge tone="amber">冷却中</Badge>
                        )}
                        {poolItem?.disabled && (
                          <Badge tone="red">已禁用</Badge>
                        )}
                        {running && poolItem && !poolItem.cooling && !poolItem.disabled && (
                          <Badge tone="green">就绪</Badge>
                        )}
                      </div>
                    </label>
                  );
                })}
              </div>

              <button
                className="btn-secondary mt-3 flex w-full items-center justify-center gap-2"
                onClick={savePool}
                disabled={savingPool}
              >
                <Save size={15} />
                {savingPool ? '保存中…' : '保存账号池'}
              </button>
            </>
          )}
        </div>
      </div>

      {/* 运行中池状态详情 */}
      {running && poolStatus.length > 0 && (
        <div className="mt-5 card p-5">
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <CheckCircle2 size={18} className="text-emerald-500" />
              <h2 className="text-sm font-semibold text-slate-800 dark:text-zinc-100">
                池实时状态
              </h2>
            </div>
            <button
              className="btn-ghost flex items-center gap-1 text-xs"
              onClick={clearAllCooldowns}
              disabled={clearingCooldowns}
              title="清除所有账号的冷却状态"
            >
              <Eraser size={13} className={clearingCooldowns ? 'animate-pulse' : ''} />
              {clearingCooldowns ? '清除中…' : '清除冷却'}
            </button>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-slate-200 text-left text-xs text-slate-500 dark:border-zinc-700 dark:text-zinc-400">
                  <th className="pb-2 pr-4 font-medium">账号</th>
                  <th className="pb-2 pr-4 font-medium">UID</th>
                  <th className="pb-2 pr-4 font-medium">积分</th>
                  <th className="pb-2 pr-4 font-medium">状态</th>
                  <th className="pb-2 pr-4 font-medium">错误次数</th>
                  <th className="pb-2 font-medium">冷却原因</th>
                </tr>
              </thead>
              <tbody>
                {poolStatus.map((p) => (
                  <tr
                    key={p.uid}
                    className="border-b border-slate-100 last:border-0 dark:border-zinc-800"
                  >
                    <td className="py-2 pr-4 font-medium text-slate-700 dark:text-zinc-200">
                      {p.name}
                    </td>
                    <td className="py-2 pr-4 text-xs text-slate-400">{p.uid.slice(0, 12)}…</td>
                    <td className="py-2 pr-4 tabular-nums text-slate-600 dark:text-zinc-300">
                      {p.credits != null ? p.credits.toFixed(0) : '—'}
                    </td>
                    <td className="py-2 pr-4">
                      {p.disabled ? (
                        <Badge tone="red">已禁用</Badge>
                      ) : p.cooling ? (
                        <Badge tone="amber">冷却中</Badge>
                      ) : (
                        <Badge tone="green">就绪</Badge>
                      )}
                    </td>
                    <td className="py-2 pr-4 tabular-nums text-slate-500">{p.err_count}</td>
                    <td className="py-2 text-xs text-slate-400">
                      {p.cooldown_reason ?? '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
