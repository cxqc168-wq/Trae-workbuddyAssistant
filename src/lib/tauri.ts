import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AccountView,
  ApiServiceStatus,
  ApiPoolFile,
  CheckinDone,
  CheckinOpts,
  CreditRecord,
  CreditsDailySnapshot,
  EnvStatus,
  GroupView,
  JwtParseResult,
  LogLine,
  OAuthLoginUrl,
  OAuthLoginResult,
  PoolStatus,
  ProfileInfo,
  ProxyLogListResult,
  ProxyStatus,
  Settings,
  WorkBuddyAccountMeta,
  WorkBuddyCheckinEntry,
  WorkBuddyCreditSummary,
} from '../types';

// 所有 invoke 封装集中于此，字段名严格遵循 Rust 端 snake_case 约定。
export const api = {
  env: {
    check: () => invoke<EnvStatus>('env_check'),
    openSite: () => invoke('open_trae_website'),
    openApp: (proxyPort?: number) => invoke('open_trae_app', { proxyPort }),
  },
  cert: {
    status: () => invoke<{ installed: boolean }>('cert_status'),
    install: () => invoke<{ installed: boolean }>('cert_install'),
  },
  proxy: {
    start: (port: number) => invoke<ProxyStatus>('proxy_start', { port }),
    stop: () => invoke<ProxyStatus>('proxy_stop'),
    status: () => invoke<ProxyStatus>('proxy_status'),
  },
  accounts: {
    list: () => invoke<AccountView[]>('accounts_list'),
    addManual: (name: string, jwt: string, groupId?: string) =>
      invoke('account_add_manual', { name, jwt, groupId }),
    delete: (userId: string, deleteProfile: boolean) =>
      invoke('account_delete', { userId, deleteProfile }),
    update: (userId: string, name?: string, jwt?: string) =>
      invoke('account_update', { userId, name, jwt }),
    fetchRemainingCredits: (userId: string) =>
      invoke<number>('fetch_remaining_credits', { userId }),
    refreshRemainingCredits: () =>
      invoke<number>('refresh_remaining_credits'),
    dailyList: () =>
      invoke<CreditsDailySnapshot[]>('credits_daily_list'),
    cooldownClear: (userId: string) =>
      invoke('cooldown_clear', { userId }),
    cooldownClearAll: () =>
      invoke<number>('cooldown_clear_all'),
    refreshJwt: (userId: string) =>
      invoke<string>('refresh_jwt', { userId }),
    exportRaw: () => invoke<Record<string, unknown>>('accounts_export_raw'),
  },
  groups: {
    list: () => invoke<GroupView[]>('groups_list'),
    create: (name: string, color: string) => invoke<string>('group_create', { name, color }),
    update: (id: string, patch: { name?: string; color?: string; order?: number }) =>
      invoke('group_update', { id, ...patch }),
    remove: (id: string) => invoke('group_delete', { id }),
    move: (userId: string, groupId: string | null) => invoke('group_move', { userId, groupId }),
  },
  checkin: {
    start: (opts: CheckinOpts) => invoke('checkin_start', { opts }),
  },
  misc: {
    deviceReset: (userId: string) => invoke('device_reset', { userId }),
    jwtParse: (jwt: string) => invoke<JwtParseResult>('jwt_parse', { jwt }),
    logsQuery: (opts: {
      logType?: string;
      date?: string;
      keyword?: string;
      limit?: number;
    }) =>
      invoke<LogLine[]>('logs_query', {
        opts: {
          log_type: opts.logType,
          date: opts.date,
          keyword: opts.keyword,
          limit: opts.limit,
        },
      }),
    settingsGet: () => invoke<Settings>('settings_get'),
    settingsSet: (patch: Settings) => invoke('settings_set', { patch }),
    creditsHistory: () => invoke<CreditRecord[]>('credits_history'),
    inviteLink: () => invoke<{ url: string }>('invite_link'),
    taskRegister: (time: string) => invoke('task_register', { time }),
    taskStatus: () => invoke<string>('task_status'),
    taskUnregister: () => invoke('task_unregister'),
    proxyLogsList: (opts: {
      keyword?: string;
      startTime?: string;
      endTime?: string;
      offset?: number;
      limit?: number;
    }) => invoke<ProxyLogListResult>('proxy_logs_list', {
      opts: {
        keyword: opts.keyword,
        start_time: opts.startTime,
        end_time: opts.endTime,
        offset: opts.offset,
        limit: opts.limit,
      },
    }),
    proxyLogDetail: (id: string) => invoke<string>('proxy_log_detail', { id }),
    writeTextFile: (path: string, content: string) =>
      invoke('write_text_file', { path, content }),
  },
  switchAccount: (userId: string) => invoke('switch_account', { userId }),
  saveCurrentLogin: (userId: string) => invoke('save_current_login', { userId }),
  resetDeviceIds: () => invoke('reset_device_ids'),
  profiles: {
    list: () => invoke<ProfileInfo[]>('profile_list'),
    backup: (userId: string) => invoke('profile_backup', { userId }),
    restore: (userId: string) => invoke('profile_restore', { userId }),
    delete: (userId: string) => invoke('profile_delete', { userId }),
    formatSize: (bytes: number) => invoke<string>('profile_format_size', { bytes }),
  },
  oauth: {
    getLoginUrl: () => invoke<OAuthLoginUrl>('oauth_get_login_url'),
    parseCallback: (callbackUrl: string) =>
      invoke('oauth_parse_callback', { callbackUrl }),
    login: (callbackUrl: string, accountName?: string, groupId?: string) =>
      invoke<OAuthLoginResult>('oauth_login', { callbackUrl, accountName, groupId }),
    callbackStart: () => invoke('oauth_callback_start'),
    callbackStop: () => invoke('oauth_callback_stop'),
  },
  apiServer: {
    start: () => invoke<ApiServiceStatus>('api_server_start'),
    stop: () => invoke('api_server_stop'),
    status: () => invoke<ApiServiceStatus>('api_server_status'),
    poolList: () => invoke<ApiPoolFile>('pool_list'),
    poolSet: (uids: string[]) => invoke('pool_set', { uids }),
    poolStatus: () => invoke<PoolStatus[]>('pool_status'),
    logsList: () => invoke<string[]>('api_logs_list'),
    logsDetail: (date: string) => invoke<string | null>('api_logs_detail', { date }),
    logsSearch: (opts: {
      date: string;
      startTime?: string;
      endTime?: string;
      keyword?: string;
    }) => invoke<string | null>('api_logs_search', {
      opts: {
        date: opts.date,
        start_time: opts.startTime,
        end_time: opts.endTime,
        keyword: opts.keyword,
      },
    }),
    debugToggle: () => invoke<boolean>('api_debug_toggle'),
    debugStatus: () => invoke<boolean>('api_debug_status'),
  },
  workbuddy: {
    listAccounts: () => invoke<WorkBuddyAccountMeta[]>('workbuddy_list_accounts'),
    clientStatus: () => invoke<{ loggedIn: boolean }>('workbuddy_client_status'),
    importLocal: () => invoke<WorkBuddyAccountMeta>('workbuddy_import_local'),
    addManual: (args: { access_token: string; refresh_token?: string; uid?: string; nickname?: string }) =>
      invoke<WorkBuddyAccountMeta>('workbuddy_add_manual', { args }),
    deleteAccount: (accountId: string) => invoke('workbuddy_delete_account', { accountId }),
    checkinStatus: (accountId: string) => invoke<{ ok: boolean; todayCheckedIn?: boolean; error?: string }>('workbuddy_checkin_status', { accountId }),
    checkinAll: (accountIds?: string[]) => invoke<WorkBuddyCheckinEntry[]>('workbuddy_checkin_all', { accountIds }),
    credits: (accountId?: string) => invoke<WorkBuddyCreditSummary[]>('workbuddy_credits', { accountId }),
    refreshToken: (accountId: string) => invoke<WorkBuddyAccountMeta>('workbuddy_refresh_token', { accountId }),
    oauthStart: () => invoke<{ loginId: string; verificationUri: string; expiresIn: number }>('workbuddy_oauth_start'),
    oauthPoll: (loginId: string) => invoke<{ done: boolean; result?: WorkBuddyAccountMeta; error?: string }>('workbuddy_oauth_poll', { loginId }),
  },
};

// ---- 事件载荷 ----
export interface CheckinStartEvent {
  type: 'start';
  total: number;
}
export interface CheckinAccountEvent {
  type: 'account';
  index: number;
  user_id: string;
  name: string;
  status: 'already' | 'success' | 'fail';
  credits?: number;
  delta?: number;
  elapsed?: number;
  code?: number;
  message?: string;
  error_type?: string | null;
  cooldown_until?: number | null;
}
export interface CheckinDoneEvent {
  type: 'done';
  ok: number;
  already: number;
  failed: number;
  total?: number;
}
export type CheckinProgressEvent =
  | CheckinStartEvent
  | CheckinAccountEvent
  | CheckinDoneEvent;

export interface SwitchDoneEvent {
  success: boolean;
  raw: string;
}

export interface SaveLoginDoneEvent {
  success: boolean;
  raw: string;
}

export interface DeviceResetDoneEvent {
  success: boolean;
  raw: string;
}

export interface ProfileDoneEvent {
  success: boolean;
  raw: string;
  action: 'backup' | 'restore';
}

export interface ListenerHandlers {
  onProxyLog?: (line: string) => void;
  onAccountCaptured?: (uid: string) => void;
  onCheckinProgress?: (e: CheckinProgressEvent) => void;
  onSwitchProgress?: (line: string) => void;
  onSwitchDone?: (e: SwitchDoneEvent) => void;
  onSaveLoginProgress?: (line: string) => void;
  onSaveLoginDone?: (e: SaveLoginDoneEvent) => void;
  onDeviceResetProgress?: (line: string) => void;
  onDeviceResetDone?: (e: DeviceResetDoneEvent) => void;
  onProfileProgress?: (line: string) => void;
  onProfileDone?: (e: ProfileDoneEvent) => void;
}

export async function setupListeners(
  handlers: ListenerHandlers,
): Promise<UnlistenFn[]> {
  const unsubs: UnlistenFn[] = [];
  if (handlers.onProxyLog) {
    unsubs.push(
      await listen<string>('proxy-log', (e) => handlers.onProxyLog!(e.payload)),
    );
  }
  if (handlers.onAccountCaptured) {
    unsubs.push(
      await listen<string>('account-captured', (e) =>
        handlers.onAccountCaptured!(e.payload),
      ),
    );
  }
  if (handlers.onCheckinProgress) {
    unsubs.push(
      await listen<CheckinProgressEvent>('checkin-progress', (e) =>
        handlers.onCheckinProgress!(e.payload),
      ),
    );
  }
  if (handlers.onSwitchProgress) {
    unsubs.push(
      await listen<string>('switch-progress', (e) =>
        handlers.onSwitchProgress!(e.payload),
      ),
    );
  }
  if (handlers.onSwitchDone) {
    unsubs.push(
      await listen<SwitchDoneEvent>('switch-done', (e) =>
        handlers.onSwitchDone!(e.payload),
      ),
    );
  }
  if (handlers.onSaveLoginProgress) {
    unsubs.push(
      await listen<string>('save-login-progress', (e) =>
        handlers.onSaveLoginProgress!(e.payload),
      ),
    );
  }
  if (handlers.onSaveLoginDone) {
    unsubs.push(
      await listen<SaveLoginDoneEvent>('save-login-done', (e) =>
        handlers.onSaveLoginDone!(e.payload),
      ),
    );
  }
  if (handlers.onDeviceResetProgress) {
    unsubs.push(
      await listen<string>('device-reset-progress', (e) =>
        handlers.onDeviceResetProgress!(e.payload),
      ),
    );
  }
  if (handlers.onDeviceResetDone) {
    unsubs.push(
      await listen<DeviceResetDoneEvent>('device-reset-done', (e) =>
        handlers.onDeviceResetDone!(e.payload),
      ),
    );
  }
  if (handlers.onProfileProgress) {
    unsubs.push(
      await listen<string>('profile-progress', (e) =>
        handlers.onProfileProgress!(e.payload),
      ),
    );
  }
  if (handlers.onProfileDone) {
    unsubs.push(
      await listen<ProfileDoneEvent>('profile-done', (e) =>
        handlers.onProfileDone!(e.payload),
      ),
    );
  }
  return unsubs;
}
