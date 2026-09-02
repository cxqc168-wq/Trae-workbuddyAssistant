import { useEffect, useState } from 'react';
import { Calendar, Power, Trash2, Save, Search, RotateCcw, Fingerprint, RefreshCw, Clock, AlertTriangle } from 'lucide-react';
import PageHeader from '../components/PageHeader';
import { Badge, Modal } from '../components/ui';
import { useAppStore } from '../store';
import { api } from '../lib/tauri';
import { withMinDelay } from '../lib/delay';
import type { Settings as SettingsType } from '../types';

export default function Settings() {
  const settings = useAppStore((s) => s.settings);
  const saveSettings = useAppStore((s) => s.saveSettings);
  const refreshSettings = useAppStore((s) => s.refreshSettings);
  const resetDeviceIds = useAppStore((s) => s.resetDeviceIds);
  const deviceResetActive = useAppStore((s) => s.deviceResetActive);
  const deviceResetProgress = useAppStore((s) => s.deviceResetProgress);
  const toast = useAppStore((s) => s.pushToast);

  const [time, setTime] = useState('09:00');
  const [taskInfo, setTaskInfo] = useState<string>('');
  const [busyTask, setBusyTask] = useState(false);
  const [querying, setQuerying] = useState(false);
  const [detecting, setDetecting] = useState(false);

  // 确认弹窗状态
  const [confirmUnregister, setConfirmUnregister] = useState(false);
  const [confirmResetDevice, setConfirmResetDevice] = useState(false);

  // 本地表单状态：用户编辑后点击「保存」才持久化，避免每次按键都写文件
  const [form, setForm] = useState<SettingsType | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    void refreshSettings();
    void query();
  }, [refreshSettings]);

  // settings 从后端加载完毕后同步到本地 form
  useEffect(() => {
    if (settings && !form) {
      setForm(settings);
    }
  }, [settings, form]);

  const dirty = form != null && settings != null && JSON.stringify(form) !== JSON.stringify(settings);

  const update = <K extends keyof SettingsType>(key: K, val: SettingsType[K]) => {
    setForm((prev) => (prev ? { ...prev, [key]: val } : prev));
  };

  const save = async () => {
    if (!form) return;
    setSaving(true);
    try {
      await withMinDelay(saveSettings(form));
      toast('success', '设置已保存');
    } catch {
      /* toast 已发出 */
    } finally {
      setSaving(false);
    }
  };

  const reset = () => {
    if (settings) setForm({ ...settings });
  };

  const register = async () => {
    setBusyTask(true);
    try {
      await withMinDelay(api.misc.taskRegister(time));
      toast('success', `已注册每日 ${time} 自动签到`);
      await query();
    } catch (e) {
      const msg = String(e);
      // 长错误信息（含换行）在 taskInfo 区域展示，toast 只给简短提示
      if (msg.includes('\n')) {
        setTaskInfo(`❌ ${msg}`);
        toast('error', '注册失败：权限不足，请查看下方详细解决方案');
      } else {
        toast('error', `注册失败：${msg}`);
      }
    } finally {
      setBusyTask(false);
    }
  };

  const unregister = async () => {
    setConfirmUnregister(false);
    setBusyTask(true);
    try {
      await withMinDelay(api.misc.taskUnregister());
      toast('info', '计划任务已删除');
      setTaskInfo('');
    } catch (e) {
      toast('error', `删除失败：${String(e)}`);
    } finally {
      setBusyTask(false);
    }
  };

  const query = async () => {
    setQuerying(true);
    try {
      const r = await withMinDelay(api.misc.taskStatus());
      setTaskInfo(r);
      toast('success', '查询完成');
    } catch (e) {
      setTaskInfo(`查询失败：${String(e)}`);
      toast('error', `查询失败：${String(e)}`);
    } finally {
      setQuerying(false);
    }
  };

  const detectTrae = async () => {
    setDetecting(true);
    try {
      const r = await withMinDelay(api.env.check());
      if (r.installed && r.path) {
        update('trae_path', r.path);
        toast('success', '已自动检测并填入 Trae Work 路径');
      } else {
        toast('info', '未检测到 Trae Work，请手动指定 exe 路径');
      }
    } catch (e) {
      toast('error', `检测失败：${String(e)}`);
    } finally {
      setDetecting(false);
    }
  };

  const handleResetDeviceIds = async () => {
    setConfirmResetDevice(false);
    await resetDeviceIds();
  };

  if (!form) {
    return (
      <div className="animate-fade-in">
        <PageHeader title="系统设置" />
        <div className="text-sm text-slate-500">加载中…</div>
      </div>
    );
  }

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="系统设置"
        desc="主题、代理端口、定时任务与邀请链接"
        actions={
          dirty ? (
            <div className="flex items-center gap-2">
              <span className="text-xs text-amber-500">有未保存的更改</span>
              <button onClick={reset} className="btn-outline">
                <RotateCcw size={15} /> 撤销
              </button>
              <button onClick={save} disabled={saving} className="btn-primary">
                <Save size={15} /> {saving ? '保存中…' : '保存'}
              </button>
            </div>
          ) : (
            <button onClick={save} disabled={saving || !dirty} className="btn-outline opacity-50">
              <Save size={15} /> 已保存
            </button>
          )
        }
      />

      <div className="grid gap-4 md:grid-cols-2">
        <section className="card p-4">
          <h3 className="mb-3 font-medium">外观</h3>
          <div className="space-y-3 text-sm">
            <div>
              <label className="label">主题</label>
              <select
                value={form.theme}
                onChange={(e) => update('theme', e.target.value)}
                className="input"
              >
                <option value="system">跟随系统</option>
                <option value="light">浅色</option>
                <option value="dark">深色</option>
              </select>
            </div>
            <div>
              <label className="label">语言</label>
              <select
                value={form.language}
                onChange={(e) => update('language', e.target.value)}
                className="input"
              >
                <option value="zh-CN">简体中文</option>
                <option value="en-US">English</option>
              </select>
            </div>
          </div>

          <div className="my-4 border-t border-slate-100 dark:border-zinc-800" />

          <h3 className="mb-3 font-medium">通用与通知</h3>
          <div className="space-y-3 text-sm">
            <div>
              <label className="label">通知方式</label>
              <select
                value={form.notify}
                onChange={(e) => update('notify', e.target.value)}
                className="input"
              >
                <option value="toast">应用内 Toast</option>
                <option value="system">系统通知</option>
                <option value="both">Toast + 系统通知</option>
                <option value="none">不通知</option>
              </select>
            </div>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.launch_minimized}
                onChange={(e) => update('launch_minimized', e.target.checked)}
              />
              启动时最小化到托盘
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.tray}
                onChange={(e) => update('tray', e.target.checked)}
              />
              启用系统托盘图标
            </label>
            <p className="text-xs text-slate-400">托盘与最小化设置变更后需重启应用生效。</p>
          </div>

          <div className="my-4 border-t border-slate-100 dark:border-zinc-800" />

          <h3 className="mb-2 font-medium">6 层设备标识重置</h3>
          <p className="mb-3 text-xs text-slate-500">
            重置 TRAE 的全部设备标识层：① machineid ② storage.json telemetry ③ storage.json aha.device ④ TinyStorage ⑤ 注册表 MachineGuid ⑥ webview 追踪数据。
            用于账号隔离和防关联，建议先关闭 TRAE 再执行。
          </p>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setConfirmResetDevice(true)}
              disabled={deviceResetActive}
              className="btn-primary"
            >
              <Fingerprint size={15} /> {deviceResetActive ? '重置中…' : '执行 6 层重置'}
            </button>
            {deviceResetActive && (
              <span className="text-xs text-amber-500 animate-pulse">正在执行，请勿关闭应用…</span>
            )}
          </div>
          {deviceResetProgress.length > 0 && (
            <pre className="mt-3 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-slate-50 p-3 text-xs dark:bg-zinc-950">
              {deviceResetProgress.join('\n')}
            </pre>
          )}

          <div className="my-4 border-t border-slate-100 dark:border-zinc-800" />

          <h3 className="mb-2 font-medium">每日定时签到</h3>
          <p className="mb-3 text-xs text-slate-500">
            通过 Windows 计划任务在指定时间自动运行 Python 签到脚本（无需启动应用界面）。
            需要管理员权限。
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative flex items-center">
              <Clock size={15} className="pointer-events-none absolute left-2.5 text-slate-400" />
              <input
                type="time"
                value={time}
                onChange={(e) => setTime(e.target.value)}
                className="input h-9 !w-32 pl-8 text-sm"
              />
            </div>
            <button onClick={register} disabled={busyTask} className="btn-primary">
              <Calendar size={15} /> {busyTask ? '注册中…' : '注册任务'}
            </button>
            <button onClick={query} disabled={querying} className="btn-outline">
              <Search size={15} /> {querying ? '查询中…' : '查询'}
            </button>
            <button onClick={() => setConfirmUnregister(true)} disabled={busyTask} className="btn-danger">
              <Trash2 size={15} /> {busyTask ? '删除中…' : '取消'}
            </button>
          </div>
          {taskInfo && (
            <pre
              className={`mt-3 overflow-auto whitespace-pre-wrap rounded-lg p-3 text-xs ${
                taskInfo.startsWith('❌')
                  ? 'max-h-80 border border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-800 dark:bg-rose-900/20 dark:text-rose-300'
                  : 'max-h-40 bg-slate-50 dark:bg-zinc-950'
              }`}
            >
              {taskInfo}
            </pre>
          )}
        </section>

        <section className="card p-4">
          <h3 className="mb-3 font-medium">代理与签到</h3>
          <div className="space-y-3 text-sm">
            <div>
              <label className="label">代理端口</label>
              <input
                type="number"
                value={form.proxy_port}
                onChange={(e) => update('proxy_port', Math.min(65535, Math.max(1, Number(e.target.value) || 8899)))}
                className="input w-32"
                min={1}
                max={65535}
              />
            </div>
            <div>
              <label className="label">Trae Work 安装路径</label>
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={form.trae_path ?? ''}
                  onChange={(e) => update('trae_path', e.target.value.trim() || null)}
                  placeholder="留空则自动检测（默认 C:\Users\你\AppData\Local\Programs\TRAE SOLO CN\TRAE SOLO CN.exe）"
                  className="input flex-1"
                />
                <button onClick={detectTrae} disabled={detecting} className="btn-outline shrink-0">
                  <Search size={15} /> {detecting ? '检测中…' : '自动检测'}
                </button>
              </div>
              <p className="mt-1 text-xs text-slate-400">
                自定义安装目录时请填写 Trae Work 的 exe 路径；留空将自动探测，并在「打开 Trae Work」时优先使用此路径。
              </p>
            </div>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.auto_start_proxy}
                onChange={(e) => update('auto_start_proxy', e.target.checked)}
              />
              启动时自动开启代理
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.checkin_skip_checked}
                onChange={(e) => update('checkin_skip_checked', e.target.checked)}
              />
              签到默认跳过今日已签
            </label>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={form.checkin_skip_expired}
                onChange={(e) => update('checkin_skip_expired', e.target.checked)}
              />
              签到默认跳过 JWT 过期
            </label>
            <div>
              <label className="label">失败重试次数</label>
              <input
                type="number"
                value={form.retry}
                onChange={(e) => update('retry', Math.min(5, Math.max(0, Number(e.target.value) || 0)))}
                className="input w-24"
                min={0}
                max={5}
              />
            </div>
            <div>
              <label className="label">日志保留天数</label>
              <input
                type="number"
                value={form.log_retention_days}
                onChange={(e) => update('log_retention_days', Math.min(365, Math.max(1, Number(e.target.value) || 30)))}
                className="input w-24"
                min={1}
                max={365}
              />
            </div>
            <div>
              <label className="label">代理监听域名列表</label>
              <textarea
                value={form.proxy_domains}
                onChange={(e) => update('proxy_domains', e.target.value)}
                className="input min-h-[60px] text-xs"
                placeholder="trae.cn,trae.com.cn,mchost.guru,zijieapi.com,bytedance.com,volcengine.com,volces.com,treecode.com"
              />
              <p className="mt-1 text-xs text-slate-400">
                逗号分隔的域名后缀列表，匹配的域名将走 MITM 解密并记录日志。未在列表中的域名请求将透明转发但不记录日志，不影响其他 App 正常上网。留空则使用默认值。修改后需重启代理生效。
              </p>
            </div>
            <div>
              <label className="label">代理抓取日志路径</label>
              <input
                type="text"
                value={form.proxy_log_path ?? ''}
                onChange={(e) => update('proxy_log_path', e.target.value.trim() || null)}
                placeholder="留空则默认 %APPDATA%\TraeWorkAssistant\logs"
                className="input"
              />
              <p className="mt-1 text-xs text-slate-400">
                代理拦截到的完整请求/响应将记录到此目录，按 100MB 滚动存储。修改后需重启代理生效。
              </p>
            </div>
          </div>
        </section>

        <section className="card p-4 md:col-span-2">
          <h3 className="mb-2 font-medium">关于</h3>
          <div className="space-y-1 text-xs text-slate-500">
            <div>应用版本：v2.2.0</div>
            <div>数据目录：<span className="font-mono">%APPDATA%\TraeWorkAssistant\</span></div>
            <div className="pl-4 text-slate-400">配置：<span className="font-mono">conf\</span> 数据：<span className="font-mono">data\</span> 日志：<span className="font-mono">logs\</span></div>
            <div>代理 Python：内置 device_proxy.py / auto_checkin.py</div>
            <div className="flex items-center gap-2 pt-1">
              <Badge tone="brand">MIT 友好</Badge>
              <Badge tone="slate">仅本地运行</Badge>
            </div>
          </div>
        </section>
      </div>

      {/* 底部悬浮保存条 */}
      {dirty && (
        <div className="mt-4 flex items-center justify-end gap-2 rounded-lg border border-amber-300 bg-amber-50 p-3 dark:border-amber-700 dark:bg-amber-900/20">
          <span className="text-sm text-amber-600 dark:text-amber-400">有未保存的更改</span>
          <button onClick={reset} className="btn-outline">
            <RotateCcw size={15} /> 撤销更改
          </button>
          <button onClick={save} disabled={saving} className="btn-primary">
            <Save size={15} /> {saving ? '保存中…' : '保存设置'}
          </button>
        </div>
      )}

      {/* 确认删除计划任务 */}
      <Modal
        open={confirmUnregister}
        onClose={() => setConfirmUnregister(false)}
        title="确认删除计划任务"
        footer={
          <>
            <button className="btn-outline" onClick={() => setConfirmUnregister(false)}>取消</button>
            <button className="btn-danger" onClick={() => void unregister()}>确认删除</button>
          </>
        }
      >
        <div className="flex items-start gap-3">
          <AlertTriangle size={20} className="mt-0.5 shrink-0 text-amber-500" />
          <div>
            <p>确认删除「TraeWorkAssistant_DailyCheckin」计划任务？</p>
            <p className="mt-2 text-xs text-slate-400">删除后将不再自动执行每日签到。</p>
          </div>
        </div>
      </Modal>

      {/* 确认执行设备标识重置 */}
      <Modal
        open={confirmResetDevice}
        onClose={() => setConfirmResetDevice(false)}
        title="确认执行 6 层设备标识重置"
        footer={
          <>
            <button className="btn-outline" onClick={() => setConfirmResetDevice(false)}>取消</button>
            <button className="btn-primary" onClick={() => void handleResetDeviceIds()}>确认重置</button>
          </>
        }
      >
        <div className="flex items-start gap-3">
          <AlertTriangle size={20} className="mt-0.5 shrink-0 text-amber-500" />
          <div>
            <p>将重置以下全部设备标识层：</p>
            <ul className="mt-2 space-y-0.5 text-xs text-slate-400">
              <li>① machineid</li>
              <li>② storage.json telemetry</li>
              <li>③ storage.json aha.device</li>
              <li>④ TinyStorage</li>
              <li>⑤ 注册表 MachineGuid</li>
              <li>⑥ webview 追踪数据</li>
            </ul>
            <p className="mt-2 text-xs text-amber-500">建议先关闭 TRAE 再执行。</p>
          </div>
        </div>
      </Modal>
    </div>
  );
}
