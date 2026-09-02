import { create } from 'zustand';
import { sendNotification } from '@tauri-apps/plugin-notification';
import { api, setupListeners, type CheckinProgressEvent, type ProfileDoneEvent, type SaveLoginDoneEvent } from './lib/tauri';
import type {
  AccountView,
  ApiServiceStatus,
  CheckinAccountResult,
  CheckinDone,
  CreditRecord,
  CreditsDailySnapshot,
  EnvStatus,
  GroupView,
  LogLine,
  ModuleKey,
  ProfileInfo,
  ProxyStatus,
  Settings,
  ViewKey,
  WorkBuddyCheckinProgress,
} from './types';

export type ToastKind = 'info' | 'success' | 'error' | 'warn';
export interface Toast {
  id: number;
  kind: ToastKind;
  msg: string;
}

export interface CheckinState {
  active: boolean;
  total: number;
  index: number;
  results: CheckinAccountResult[];
  done: CheckinDone | null;
}

export interface WbCheckinState {
  running: boolean;
  results: WorkBuddyCheckinProgress[];
}

export interface LogQuery {
  logType?: string;
  date?: string;
  keyword?: string;
  limit?: number;
}

interface AppState {
  ready: boolean;
  view: ViewKey;
  module: ModuleKey;
  wbCheckin: WbCheckinState;
  env: EnvStatus | null;
  certInstalled: boolean;
  proxy: ProxyStatus;
  apiStatus: ApiServiceStatus | null;
  accounts: AccountView[];
  groups: GroupView[];
  settings: Settings | null;
  logs: LogLine[];
  creditsHistory: CreditRecord[];
  creditsDaily: CreditsDailySnapshot[];
  proxyLog: string[];
  switchProgress: string[];
  switchingTo: string | null;
  saveLoginProgress: string[];
  savingLogin: string | null;
  deviceResetProgress: string[];
  deviceResetActive: boolean;
  checkin: CheckinState;
  toasts: Toast[];
  profiles: ProfileInfo[];
  profileProgress: string[];
  profileActive: boolean;

  init: () => Promise<void>;
  setView: (v: ViewKey) => void;
  setModule: (m: ModuleKey) => void;
  setWbCheckin: (partial: Partial<WbCheckinState>) => void;
  applyCheckinEvent: (e: CheckinProgressEvent) => void;

  refreshEnv: () => Promise<void>;
  refreshCert: () => Promise<void>;
  refreshProxy: () => Promise<void>;
  refreshApiStatus: () => Promise<void>;
  refreshAccounts: () => Promise<void>;
  refreshGroups: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshLogs: (q?: LogQuery) => Promise<void>;
  refreshCreditsHistory: () => Promise<void>;
  refreshCreditsDaily: () => Promise<void>;
  refreshProfiles: () => Promise<void>;

  startProxy: () => Promise<void>;
  stopProxy: () => Promise<void>;
  openTraeWithProxy: () => Promise<void>;
  addAccount: (name: string, jwt: string, groupId?: string) => Promise<void>;
  deleteAccount: (userId: string, deleteProfile: boolean) => Promise<void>;
  updateAccount: (userId: string, name?: string, jwt?: string) => Promise<void>;
  createGroup: (name: string, color: string) => Promise<void>;
  updateGroup: (
    id: string,
    patch: { name?: string; color?: string; order?: number },
  ) => Promise<void>;
  removeGroup: (id: string) => Promise<void>;
  moveAccount: (userId: string, groupId: string | null) => Promise<void>;
  resetDevice: (userId: string) => Promise<void>;
  switchTo: (userId: string) => Promise<void>;
  saveCurrentLogin: (userId: string) => Promise<void>;
  renewJwt: (userId: string) => Promise<void>;
  resetDeviceIds: () => Promise<void>;
  startCheckin: (opts: {
    scope: string;
    user_ids?: string[];
    skip_checked_in: boolean;
    skip_expired: boolean;
  }) => Promise<void>;
  refreshRemainingCredits: () => Promise<void>;
  cooldownClear: (userId: string) => Promise<void>;
  refreshJwt: (userId: string) => Promise<void>;
  saveSettings: (patch: Partial<Settings>) => Promise<void>;
  profileBackup: (userId: string) => Promise<void>;
  profileRestore: (userId: string) => Promise<void>;
  profileDelete: (userId: string) => Promise<void>;
  oauthLogin: (callbackUrl: string, accountName?: string, groupId?: string) => Promise<void>;

  pushToast: (kind: ToastKind, msg: string) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 0;
// 已注册的事件监听取消函数；StrictMode 下 init 会执行两次，靠它先注销旧监听避免重复注册
let unsubs: Array<() => void> = [];

function defaultSettings(): Settings {
  return {
    proxy_port: 8899,
    theme: 'system',
    launch_minimized: false,
    auto_start_proxy: true,
    tray: true,
    language: 'zh-CN',
    checkin_skip_checked: true,
    checkin_skip_expired: true,
    retry: 1,
    notify: 'toast',
    trae_path: null,
    data_dir: null,
    log_retention_days: 30,
    proxy_domains: 'trae.cn,trae.com.cn,mchost.guru,zijieapi.com,bytedance.com,volcengine.com,volces.com,treecode.com',
    proxy_log_path: null,
    api_port: 7864,
    api_key: '',
    api_default_model: 'deepseek-v4-flash',
  };
}

export const useAppStore = create<AppState>((set, get) => ({
  ready: false,
  view: 'dashboard',
  module: 'trae',
  wbCheckin: { running: false, results: [] },
  env: null,
  certInstalled: false,
  proxy: { running: false, port: 0, captured: 0, started_at: null },
  apiStatus: null,
  accounts: [],
  groups: [],
  settings: null,
  logs: [],
  creditsHistory: [],
  creditsDaily: [],
  proxyLog: [],
  switchProgress: [],
  switchingTo: null,
  saveLoginProgress: [],
  savingLogin: null,
  deviceResetProgress: [],
  deviceResetActive: false,
  checkin: { active: false, total: 0, index: 0, results: [], done: null },
  toasts: [],
  profiles: [],
  profileProgress: [],
  profileActive: false,

  init: async () => {
    // StrictMode 下 effect 会执行两次：先注销旧监听，避免重复注册导致事件触发两次（如 captured 重复 +1、toast 双发）
    unsubs.forEach((u) => u());
    unsubs = [];
    unsubs = await setupListeners({
      onProxyLog: (line) =>
        set((s) => ({ proxyLog: [line, ...s.proxyLog.slice(0, 999)] })),
      onAccountCaptured: (uid) => {
        // 事件驱动累加捕获数（后端 Arc<AtomicI64> 的实时镜像，避免轮询）
        set((s) => ({ proxy: { ...s.proxy, captured: s.proxy.captured + 1 } }));
        get().pushToast('success', `已捕获账号 ${uid}`);
        void get().refreshAccounts();
      },
      onCheckinProgress: (e) => get().applyCheckinEvent(e),
      onSwitchProgress: (line) =>
        set((s) => ({ switchProgress: [...s.switchProgress.slice(-49), line] })),
      // D2：订阅后端 switch-done，给用户明确的切换完成/失败信号
      onSwitchDone: (e) => {
        set((s) => ({
          switchingTo: null,
          switchProgress: [
            ...s.switchProgress.slice(-49),
            e.success ? '[完成] 登录态切换成功' : '[失败] 登录态切换未完成，请查看日志',
          ],
        }));
        get().pushToast(
          e.success ? 'success' : 'error',
          e.success ? '登录态切换完成' : '登录态切换失败，请查看日志',
        );
        void get().refreshAccounts();
        void get().refreshProxy();
      },
      onSaveLoginProgress: (line) =>
        set((s) => ({ saveLoginProgress: [...s.saveLoginProgress.slice(-49), line] })),
      onSaveLoginDone: (e: SaveLoginDoneEvent) => {
        set((s) => ({
          savingLogin: null,
          saveLoginProgress: [
            ...s.saveLoginProgress.slice(-49),
            e.success ? '[完成] 登录态保存成功' : '[失败] 登录态保存失败，请查看日志',
          ],
        }));
        get().pushToast(
          e.success ? 'success' : 'error',
          e.success ? '登录态已保存，可随时切换回此账号' : '登录态保存失败，请查看日志',
        );
        void get().refreshProfiles();
      },
      onDeviceResetProgress: (line) =>
        set((s) => ({ deviceResetProgress: [...s.deviceResetProgress.slice(-99), line] })),
      onDeviceResetDone: (e) => {
        set((s) => ({
          deviceResetActive: false,
          deviceResetProgress: [
            ...s.deviceResetProgress.slice(-99),
            e.success ? '[完成] 6 层设备标识重置成功' : '[失败] 设备标识重置未完成，请查看日志',
          ],
        }));
        get().pushToast(
          e.success ? 'success' : 'error',
          e.success ? '6 层设备标识重置完成' : '设备标识重置失败，请查看日志',
        );
      },
      onProfileProgress: (line) =>
        set((s) => ({ profileProgress: [...s.profileProgress.slice(-49), line] })),
      onProfileDone: (e: ProfileDoneEvent) => {
        set((s) => ({
          profileActive: false,
          profileProgress: [
            ...s.profileProgress.slice(-49),
            e.success ? `[完成] ${e.action === 'backup' ? '备份' : '恢复'}成功` : `[失败] ${e.action === 'backup' ? '备份' : '恢复'}失败`,
          ],
        }));
        get().pushToast(
          e.success ? 'success' : 'error',
          e.success
            ? `登录态${e.action === 'backup' ? '备份' : '恢复'}完成`
            : `登录态${e.action === 'backup' ? '备份' : '恢复'}失败`,
        );
        void get().refreshProfiles();
      },
    });
    await Promise.all([
      get().refreshEnv(),
      get().refreshCert(),
      get().refreshProxy(),
      get().refreshApiStatus(),
      get().refreshAccounts(),
      get().refreshGroups(),
      get().refreshSettings(),
      get().refreshCreditsHistory(),
      get().refreshCreditsDaily(),
      get().refreshProfiles(),
    ]);
    set({ ready: true });

    // 启动时根据设置自动开启代理
    const s = get();
    if (!s.proxy.running && s.settings?.auto_start_proxy) {
      void s.startProxy();
    }
  },

  setView: (v) => set({ view: v }),

  setModule: (m) => {
    if (m === get().module) return;
    set({
      module: m,
      view: m === 'trae' ? 'dashboard' : 'wb-accounts',
    });
  },
  setWbCheckin: (partial) =>
    set((s) => ({ wbCheckin: { ...s.wbCheckin, ...partial } })),

  applyCheckinEvent: (e) => {
    set((s) => {
      if (e.type === 'start') {
        return {
          checkin: { active: true, total: e.total, index: 0, results: [], done: null },
        };
      }
      if (e.type === 'account') {
        const results = s.checkin.results.slice();
        const i = e.index - 1;
        results[i] = {
          index: e.index,
          user_id: e.user_id,
          name: e.name,
          status: e.status,
          credits: e.credits,
          delta: e.delta,
          elapsed: e.elapsed,
          code: e.code,
          message: e.message,
          error_type: e.error_type,
          cooldown_until: e.cooldown_until,
        };
        return { checkin: { ...s.checkin, index: e.index, results } };
      }
      return {
        checkin: {
          ...s.checkin,
          active: false,
          done: { ok: e.ok, already: e.already, failed: e.failed, total: e.total },
        },
      };
    });
    if (e.type === 'done') {
      void get().refreshAccounts();
      // 签到完成后静默刷新剩余积分（内部会再次 refreshAccounts）
      void api.accounts.refreshRemainingCredits().then(() => {
        get().refreshAccounts();
        get().refreshCreditsDaily();
      }).catch(() => {});
      get().pushToast(
        e.failed > 0 ? 'warn' : 'success',
        `签到完成：成功 ${e.ok}，已签 ${e.already}，失败 ${e.failed}`,
      );
    }
  },

  refreshEnv: async () => {
    try {
      const env = await api.env.check();
      set({ env });
    } catch (err) {
      get().pushToast('error', `环境检测失败：${String(err)}`);
    }
  },
  refreshCert: async () => {
    try {
      const r = await api.cert.status();
      set({ certInstalled: r.installed });
    } catch {
      /* ignore */
    }
  },
  refreshProxy: async () => {
    try {
      const proxy = await api.proxy.status();
      set({ proxy });
    } catch {
      /* ignore */
    }
  },
  refreshApiStatus: async () => {
    try {
      const s = await api.apiServer.status();
      set({ apiStatus: s });
    } catch {
      set({ apiStatus: null });
    }
  },
  refreshAccounts: async () => {
    try {
      const accounts = await api.accounts.list();
      set({ accounts });
    } catch (err) {
      get().pushToast('error', `读取账号失败：${String(err)}`);
    }
  },
  refreshGroups: async () => {
    try {
      const groups = await api.groups.list();
      set({ groups });
    } catch {
      /* ignore */
    }
  },
  refreshSettings: async () => {
    try {
      const settings = await api.misc.settingsGet();
      set({ settings });
    } catch {
      set({ settings: defaultSettings() });
    }
  },
  refreshLogs: async (q) => {
    try {
      const logs = await api.misc.logsQuery({
        logType: q?.logType,
        date: q?.date,
        keyword: q?.keyword,
        limit: q?.limit ?? 500,
      });
      set({ logs });
    } catch (err) {
      get().pushToast('error', `读取日志失败：${String(err)}`);
    }
  },
  refreshCreditsHistory: async () => {
    try {
      const creditsHistory = await api.misc.creditsHistory();
      set({ creditsHistory });
    } catch (err) {
      get().pushToast('error', `读取积分历史失败：${String(err)}`);
    }
  },
  refreshCreditsDaily: async () => {
    try {
      const creditsDaily = await api.accounts.dailyList();
      set({ creditsDaily });
    } catch {
      /* ignore */
    }
  },

  startProxy: async () => {
    const port = get().settings?.proxy_port || 8899;
    try {
      const proxy = await api.proxy.start(port);
      set({ proxy, proxyLog: [] });
      get().pushToast('success', `代理已启动（端口 ${proxy.port}）`);
    } catch (err) {
      get().pushToast('error', `启动代理失败：${String(err)}`);
    }
  },
  stopProxy: async () => {
    try {
      const proxy = await api.proxy.stop();
      set({ proxy });
      get().pushToast('info', '代理已停止');
    } catch (err) {
      get().pushToast('error', `停止代理失败：${String(err)}`);
    }
  },
  openTraeWithProxy: async () => {
    const env = get().env;
    if (!env?.installed) {
      get().pushToast('info', '未检测到 Trae Work 安装，正在打开下载页…');
      try {
        await api.env.openSite();
      } catch (err) {
        get().pushToast('error', `打开下载页失败：${String(err)}`);
      }
      return;
    }
    // 确保代理在请求路径上：未运行则先启动，否则打开 Trae Work 也不会走代理、无法捕获账号
    let port: number | undefined = get().proxy.running ? get().proxy.port : undefined;
    if (!port) {
      // 可能残留端口为 0 的无效代理，先停掉再以有效端口重启
      if (get().proxy.running) {
        try { await get().stopProxy(); } catch { /* ignore */ }
      }
      get().pushToast('info', '正在启动代理以确保 Trae Work 走本地代理…');
      await get().startProxy();
      port = get().proxy.running ? get().proxy.port : undefined;
    }
    try {
      if (port) {
        await api.env.openApp(port);
        get().pushToast('success', `已打开 Trae Work（代理已注入 127.0.0.1:${port}）`);
      } else {
        // 代理启动失败：仍打开客户端，但明确告知不会捕获账号
        await api.env.openApp(undefined);
        get().pushToast('warn', '代理启动失败，已直接打开 Trae Work（账号不会被自动捕获）');
      }
    } catch (err) {
      get().pushToast('error', `打开 Trae Work 失败：${String(err)}`);
    }
  },
  addAccount: async (name, jwt, groupId) => {
    try {
      await api.accounts.addManual(name, jwt, groupId);
      await get().refreshAccounts();
      get().pushToast('success', `账号「${name}」已添加`);
    } catch (err) {
      get().pushToast('error', `添加失败：${String(err)}`);
      throw err;
    }
  },
  deleteAccount: async (userId, deleteProfile) => {
    try {
      await api.accounts.delete(userId, deleteProfile);
      await get().refreshAccounts();
      get().pushToast('info', '账号已删除');
    } catch (err) {
      get().pushToast('error', `删除失败：${String(err)}`);
    }
  },
  updateAccount: async (userId, name, jwt) => {
    try {
      await api.accounts.update(userId, name, jwt);
      await get().refreshAccounts();
      get().pushToast('success', '账号已更新');
    } catch (err) {
      get().pushToast('error', `更新失败：${String(err)}`);
      throw err;
    }
  },
  createGroup: async (name, color) => {
    try {
      await api.groups.create(name, color);
      await get().refreshGroups();
      get().pushToast('success', `分组「${name}」已创建`);
    } catch (err) {
      get().pushToast('error', `创建分组失败：${String(err)}`);
    }
  },
  updateGroup: async (id, patch) => {
    try {
      await api.groups.update(id, patch);
      await get().refreshGroups();
    } catch (err) {
      get().pushToast('error', `更新分组失败：${String(err)}`);
    }
  },
  removeGroup: async (id) => {
    try {
      await api.groups.remove(id);
      await get().refreshGroups();
      await get().refreshAccounts();
      get().pushToast('info', '分组已删除');
    } catch (err) {
      get().pushToast('error', `删除分组失败：${String(err)}`);
    }
  },
  moveAccount: async (userId, groupId) => {
    try {
      await api.groups.move(userId, groupId);
      await get().refreshAccounts();
    } catch (err) {
      get().pushToast('error', `移动分组失败：${String(err)}`);
    }
  },
  resetDevice: async (userId) => {
    try {
      await api.misc.deviceReset(userId);
      await get().refreshAccounts();
      get().pushToast('success', '设备 ID 已重置');
    } catch (err) {
      get().pushToast('error', `重置失败：${String(err)}`);
    }
  },
  switchTo: async (userId) => {
    try {
      set({ switchingTo: userId, switchProgress: [] });
      await api.switchAccount(userId);
      get().pushToast('info', '正在切换登录态，请稍候…');
    } catch (err) {
      set({ switchingTo: null });
      get().pushToast('error', `切换失败：${String(err)}`);
    }
  },
  saveCurrentLogin: async (userId) => {
    try {
      set({ savingLogin: userId, saveLoginProgress: [] });
      await api.saveCurrentLogin(userId);
      get().pushToast('info', '正在保存当前登录态，请稍候…');
    } catch (err) {
      set({ savingLogin: null });
      get().pushToast('error', `保存登录态失败：${String(err)}`);
    }
  },
  renewJwt: async (userId) => {
    try {
      // 若代理未运行则先启动
      if (!get().proxy.running) {
        get().pushToast('info', '正在启动代理以续期 JWT…');
        await get().startProxy();
      }
      // 切换到目标账号，TRAE 重启后走代理，新 JWT 会被自动捕获
      get().pushToast('info', '正在切换账号以捕获新 JWT，请稍候…');
      await api.switchAccount(userId);
    } catch (err) {
      get().pushToast('error', `续期失败：${String(err)}`);
    }
  },
  resetDeviceIds: async () => {
    set({ deviceResetActive: true, deviceResetProgress: [] });
    try {
      await api.resetDeviceIds();
      get().pushToast('info', '正在执行 6 层设备标识重置…');
    } catch (err) {
      set({ deviceResetActive: false });
      get().pushToast('error', `设备标识重置失败：${String(err)}`);
    }
  },
  startCheckin: async (opts) => {
    // 重置签到状态，避免显示上一次的进度
    set({ checkin: { active: true, total: 0, index: 0, results: [], done: null } });
    try {
      await api.checkin.start(opts);
    } catch (err) {
      set((s) => ({ checkin: { ...s.checkin, active: false } }));
      get().pushToast('error', `发起签到失败：${String(err)}`);
    }
  },
  refreshRemainingCredits: async () => {
    try {
      const ok = await api.accounts.refreshRemainingCredits();
      await get().refreshAccounts();
      if (ok > 0) {
        get().pushToast('success', `已刷新 ${ok} 个账号的剩余积分`);
      }
    } catch (err) {
      get().pushToast('error', `刷新剩余积分失败：${String(err)}`);
    }
  },
  cooldownClear: async (userId) => {
    try {
      await api.accounts.cooldownClear(userId);
      await get().refreshAccounts();
      get().pushToast('success', '已解除冷却');
    } catch (err) {
      get().pushToast('error', `解除冷却失败：${String(err)}`);
    }
  },
  refreshJwt: async (userId) => {
    try {
      await api.accounts.refreshJwt(userId);
      await get().refreshAccounts();
      get().pushToast('success', 'JWT 已自动刷新');
    } catch (err) {
      get().pushToast('error', `JWT 刷新失败：${String(err)}`);
    }
  },
  saveSettings: async (patch) => {
    const current = get().settings ?? defaultSettings();
    const next = { ...current, ...patch } as Settings;
    set({ settings: next });
    try {
      await api.misc.settingsSet(next);
    } catch (err) {
      // 回滚到修改前的值，避免 UI 显示与后端不一致
      set({ settings: current });
      get().pushToast('error', `保存设置失败：${String(err)}`);
    }
  },

  refreshProfiles: async () => {
    try {
      const profiles = await api.profiles.list();
      set({ profiles });
    } catch {
      /* ignore */
    }
  },
  profileBackup: async (userId) => {
    set({ profileActive: true, profileProgress: [] });
    try {
      await api.profiles.backup(userId);
      get().pushToast('info', '正在备份登录态快照…');
    } catch (err) {
      set({ profileActive: false });
      get().pushToast('error', `备份失败：${String(err)}`);
    }
  },
  profileRestore: async (userId) => {
    set({ profileActive: true, profileProgress: [] });
    try {
      await api.profiles.restore(userId);
      get().pushToast('info', '正在恢复登录态快照…');
    } catch (err) {
      set({ profileActive: false });
      get().pushToast('error', `恢复失败：${String(err)}`);
    }
  },
  profileDelete: async (userId) => {
    try {
      await api.profiles.delete(userId);
      await get().refreshProfiles();
      get().pushToast('info', '快照已删除');
    } catch (err) {
      get().pushToast('error', `删除失败：${String(err)}`);
    }
  },
  oauthLogin: async (callbackUrl, accountName, groupId) => {
    try {
      const result = await api.oauth.login(callbackUrl, accountName, groupId);
      await get().refreshAccounts();
      get().pushToast('success', `OAuth 登录成功：账号「${result.name}」已添加`);
    } catch (err) {
      get().pushToast('error', `OAuth 登录失败：${String(err)}`);
      throw err;
    }
  },

  pushToast: (kind, msg) => {
    const mode = get().settings?.notify ?? 'toast';
    if (mode === 'none') {
      console.debug('[notify] 已跳过（mode=none）:', kind, msg);
      return;
    }

    if (mode === 'toast' || mode === 'both') {
      const id = ++toastSeq;
      set((s) => ({ toasts: [...s.toasts, { id, kind, msg }] }));
      setTimeout(() => get().dismissToast(id), 4200);
    }

    if (mode === 'system' || mode === 'both') {
      // sendNotification v2 返回 void（fire-and-forget），用 try-catch 防御同步异常
      try {
        sendNotification({ title: 'Trae Work 助手', body: msg });
        console.debug('[notify] 系统通知已发送:', msg);
      } catch (e) {
        console.warn('[notify] sendNotification 异常:', e);
      }
    }
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
