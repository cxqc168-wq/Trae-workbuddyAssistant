import { useState } from 'react';
import { CheckCircle2, Circle, ChevronRight, Info } from 'lucide-react';
import { useAppStore } from '../store';
import { api } from '../lib/tauri';
import { Badge } from './ui';

interface Step {
  key: string;
  title: string;
  desc: string;
  done: boolean;
  actionLabel: string;
  run: () => Promise<void> | void;
}

export default function SetupGuide() {
  const env = useAppStore((s) => s.env);
  const certInstalled = useAppStore((s) => s.certInstalled);
  const proxy = useAppStore((s) => s.proxy);
  const accounts = useAppStore((s) => s.accounts);
  const setView = useAppStore((s) => s.setView);
  const startProxy = useAppStore((s) => s.startProxy);
  const refreshEnv = useAppStore((s) => s.refreshEnv);
  const refreshCert = useAppStore((s) => s.refreshCert);

  const [busy, setBusy] = useState<string | null>(null);

  // 每一步的「完成」判定均来自 store 实时状态，组件重渲染时自动刷新
  const steps: Step[] = [
    {
      key: 'install',
      title: '安装 Trae Work 客户端',
      desc: '签到目标客户端，需先安装并登录至少一个账号。',
      done: !!env?.installed,
      actionLabel: env?.installed ? '打开 Trae Work' : '前往下载',
      run: async () => {
        await useAppStore.getState().openTraeWithProxy();
        await refreshEnv();
      },
    },
    {
      key: 'cert',
      title: '安装 CA 证书',
      desc: '代理需系统信任其证书，才能拦截并改写签到接口。',
      done: certInstalled,
      actionLabel: '安装证书',
      run: async () => {
        await api.cert.install();
        await refreshCert();
      },
    },
    {
      key: 'proxy',
      title: '启动代理',
      desc: '代理负责捕获登录态、转发并拦截签到请求。',
      done: proxy.running,
      actionLabel: '启动代理',
      run: async () => {
        await startProxy();
      },
    },
    {
      key: 'account',
      title: '添加账号',
      desc: '至少添加一个 Trae Work 账号，才能执行签到。',
      done: accounts.length > 0,
      actionLabel: '去添加',
      run: () => setView('accounts'),
    },
    {
      key: 'checkin',
      title: '完成首次签到',
      desc: '验证整条链路（代理 → 签到 → 浏览器）是否跑通。',
      done: accounts.some((a) => a.checked_today),
      actionLabel: '去签到',
      run: () => setView('checkin'),
    },
  ];

  const completed = steps.filter((s) => s.done).length;
  const allDone = completed === steps.length;

  // 未完成 + 当前非 busy 才允许执行；校验失败（done=true）时按钮不渲染，天然禁止重复处理
  const handleRun = async (step: Step) => {
    if (step.done || busy) return;
    setBusy(step.key);
    try {
      await step.run();
    } catch (e) {
      // 错误已由各 step.run() 内部 toast 处理；此处兜底防止未捕获异常
      console.error(`[SetupGuide] step "${step.key}" failed:`, e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="card overflow-hidden">
      <div className="flex items-center justify-between border-b border-slate-100 px-4 py-3 dark:border-zinc-800">
        <div>
          <h3 className="font-medium">配置导航</h3>
          <p className="text-xs text-slate-500">
            按步骤完成初始化，已完成的步骤无需重复处理。
          </p>
        </div>
        <Badge tone={allDone ? 'green' : 'amber'}>
          {completed}/{steps.length} 已完成
        </Badge>
      </div>

      <ol className="divide-y divide-slate-100 dark:divide-slate-800">
        {steps.map((step, i) => (
          <li key={step.key} className="flex items-center gap-3 px-4 py-3">
            <div className={step.done ? 'text-emerald-500' : 'text-slate-300 dark:text-zinc-600'}>
              {step.done ? <CheckCircle2 size={20} /> : <Circle size={20} />}
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-medium text-slate-800 dark:text-zinc-100">
                {i + 1}. {step.title}
              </div>
              <div className="text-xs text-slate-500">{step.desc}</div>
            </div>

            {step.done ? (
              <span className="shrink-0 rounded-full bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-600 dark:bg-emerald-500/15 dark:text-emerald-400">
                已完成
              </span>
            ) : (
              <button
                onClick={() => void handleRun(step)}
                disabled={busy === step.key}
                className="btn-primary shrink-0"
              >
                {busy === step.key ? '处理中…' : step.actionLabel}
                <ChevronRight size={14} />
              </button>
            )}
          </li>
        ))}
      </ol>

      {proxy.running && proxy.captured === 0 && (
        <div className="border-t border-slate-100 bg-amber-50/60 px-4 py-3 text-sm text-amber-700 dark:border-zinc-800 dark:bg-amber-500/10 dark:text-amber-300">
          <div className="mb-1 flex items-center gap-1.5 font-medium">
            <Info size={14} /> 代理已启动但尚未捕获到账号
          </div>
          <div className="space-y-1 text-xs">
            <p>
              点击上方「打开 Trae Work」启动客户端即可（代理运行时会自动注入{' '}
              <code className="rounded bg-amber-100 px-1 py-0.5 font-mono text-amber-800 dark:bg-amber-900/40 dark:text-amber-200">
                127.0.0.1:{proxy.port}
              </code>{' '}
              代理，无需手动配置）。
            </p>
            <p>2. 在 Trae Work 中登录账号，授权头经过代理后会自动写入「账号管理」。</p>
            <p>3. 若仍无账号，请确认 CA 证书已安装并信任。</p>
          </div>
        </div>
      )}

      {allDone && (
        <div className="border-t border-slate-100 bg-emerald-50/60 px-4 py-3 text-sm text-emerald-700 dark:border-zinc-800 dark:bg-emerald-500/10 dark:text-emerald-300">
          🎉 全部配置已完成，去「一键签到」开始使用吧！
        </div>
      )}
    </div>
  );
}
