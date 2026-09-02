import { useCallback, useEffect, useState } from 'react';
import {
  ExternalLink,
  Power,
  PowerOff,
  ShieldCheck,
  ShieldAlert,
  MonitorCheck,
  MonitorX,
  Server,
  Wifi,
  PlayCircle,
  Loader2,
  UserCheck,
  UserX,
  Users,
  CalendarCheck,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-shell';
import { useAppStore } from '../store';
import { api } from '../lib/tauri';
import { Badge } from './ui';
import type { WorkBuddyAccountMeta } from '../types';

/** 顶栏按左侧当前模块切换：TRAE 显示 Trae 状态栏，WorkBuddy 显示 WB 状态栏（全局页跟随当前模块）。 */
export default function TopBar() {
  const module = useAppStore((s) => s.module);
  return module === 'workbuddy' ? <WorkBuddyBar /> : <TraeBar />;
}

function TraeBar() {
  const env = useAppStore((s) => s.env);
  const certInstalled = useAppStore((s) => s.certInstalled);
  const proxy = useAppStore((s) => s.proxy);
  const apiStatus = useAppStore((s) => s.apiStatus);
  const startProxy = useAppStore((s) => s.startProxy);
  const stopProxy = useAppStore((s) => s.stopProxy);

  const openTrae = async () => {
    await useAppStore.getState().openTraeWithProxy();
  };

  return (
    <div className="flex h-12 shrink-0 items-center justify-between border-b border-slate-200 bg-white px-4 dark:border-zinc-800 dark:bg-zinc-950">
      <div className="flex items-center gap-2">
        {env?.installed ? (
          <Badge tone="green">
            <MonitorCheck size={13} /> Trae Work 已安装
            {env.version ? ` v${env.version}` : ''}
          </Badge>
        ) : (
          <Badge tone="red">
            <MonitorX size={13} /> Trae Work 未安装
          </Badge>
        )}
        {certInstalled ? (
          <Badge tone="green">
            <ShieldCheck size={13} /> 证书已信任
          </Badge>
        ) : (
          <Badge tone="amber">
            <ShieldAlert size={13} /> 证书未安装
          </Badge>
        )}
        <Badge tone={proxy.running ? 'blue' : 'slate'}>
          <Wifi size={13} /> {proxy.running ? `代理运行中 :${proxy.port}` : '代理未启动'}
        </Badge>
        <Badge tone={apiStatus?.running ? 'green' : 'slate'}>
          <Server size={13} /> {apiStatus?.running ? `API 服务 :${apiStatus.port}` : 'API 服务未启动'}
        </Badge>
      </div>
      <div className="flex items-center gap-2">
        <button onClick={openTrae} className="btn-outline">
          <ExternalLink size={15} /> {env?.installed ? '打开 Trae Work' : '下载 Trae Work'}
        </button>
        {proxy.running ? (
          <button onClick={stopProxy} className="btn-outline">
            <PowerOff size={15} /> 停止代理
          </button>
        ) : (
          <button onClick={startProxy} className="btn-primary">
            <Power size={15} /> 启动代理
          </button>
        )}
      </div>
    </div>
  );
}

function WorkBuddyBar() {
  const wbClientLoggedIn = useAppStore((s) => s.wbClientLoggedIn);
  const apiStatus = useAppStore((s) => s.apiStatus);
  const checkinRunning = useAppStore((s) => s.wbCheckin.running);
  const setWbCheckin = useAppStore((s) => s.setWbCheckin);
  const toast = useAppStore((s) => s.pushToast);

  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[] | null>(null);

  const reload = useCallback(async () => {
    try {
      setAccounts(await api.workbuddy.listAccounts());
    } catch (e) {
      toast('error', `读取 WorkBuddy 账号失败：${String(e)}`);
    }
  }, [toast]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const total = accounts?.length ?? 0;
  const checkedToday = accounts?.filter((a) => a.checkedToday).length ?? 0;
  const needsRelogin = accounts?.filter((a) => a.needsRelogin).length ?? 0;
  // 顶栏一键签到固定跳过今日已签账号
  const pendingIds = (accounts ?? []).filter((a) => !a.checkedToday).map((a) => a.id);

  const openWorkBuddy = async () => {
    try {
      await open('https://www.workbuddy.cn');
    } catch (e) {
      toast('error', `打开 WorkBuddy 失败：${String(e)}`);
    }
  };

  const startCheckinAll = async () => {
    if (pendingIds.length === 0) return;
    setWbCheckin({ running: true, results: [] });
    try {
      // 命令阻塞至全部完成；toast 由发起方（此处）负责，避免与签到页事件监听重复
      const entries = await api.workbuddy.checkinAll(pendingIds);
      setWbCheckin({
        results: entries.map((e, i) => ({ ...e, index: i, total: entries.length })),
        running: false,
      });
      const ok = entries.filter((e) => e.result === 'success').length;
      const already = entries.filter((e) => e.result === 'already').length;
      const failed = entries.length - ok - already;
      toast(
        failed > 0 ? 'warn' : 'success',
        `签到完成：成功 ${ok}，已签 ${already}，失败 ${failed}`,
      );
    } catch (e) {
      setWbCheckin({ running: false });
      toast('error', `签到失败：${String(e)}`);
    }
    void reload();
  };

  return (
    <div className="flex h-12 shrink-0 items-center justify-between border-b border-slate-200 bg-white px-4 dark:border-zinc-800 dark:bg-zinc-950">
      <div className="flex items-center gap-2">
        {wbClientLoggedIn ? (
          <Badge tone="green">
            <UserCheck size={13} /> WorkBuddy 客户端已登录
          </Badge>
        ) : (
          <Badge tone="red">
            <UserX size={13} /> WorkBuddy 客户端未登录
          </Badge>
        )}
        {accounts == null ? (
          <Badge tone="slate">
            <Users size={13} /> 账号读取中…
          </Badge>
        ) : needsRelogin > 0 ? (
          <Badge tone="amber">
            <ShieldAlert size={13} /> {total} 个账号 · {needsRelogin} 个需重登
          </Badge>
        ) : (
          <Badge tone="green">
            <ShieldCheck size={13} /> 已登录 {total} 个账号
          </Badge>
        )}
        <Badge tone={total > 0 && checkedToday === total ? 'green' : total > 0 ? 'blue' : 'slate'}>
          <CalendarCheck size={13} /> 今日已签 {checkedToday}/{total}
        </Badge>
        <Badge tone={apiStatus?.running ? 'green' : 'slate'}>
          <Server size={13} /> {apiStatus?.running ? `API 服务 :${apiStatus.port}` : 'API 服务未启动'}
        </Badge>
      </div>
      <div className="flex items-center gap-2">
        <button onClick={() => void openWorkBuddy()} className="btn-outline">
          <ExternalLink size={15} /> 打开 WorkBuddy
        </button>
        <button
          onClick={() => void startCheckinAll()}
          disabled={checkinRunning || pendingIds.length === 0}
          className="btn-primary"
        >
          {checkinRunning ? (
            <Loader2 size={15} className="animate-spin" />
          ) : (
            <PlayCircle size={15} />
          )}
          {checkinRunning
            ? '签到进行中…'
            : total === 0
              ? '一键签到'
              : pendingIds.length === 0
                ? '今日已全部签到'
                : `一键签到（${pendingIds.length}）`}
        </button>
      </div>
    </div>
  );
}
