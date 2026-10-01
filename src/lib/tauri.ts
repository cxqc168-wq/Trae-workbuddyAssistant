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
  WbStoredAuth,
  WbAuthInfo,
  WbSwitchResult,
  WbSession,
  WbSessionJob,
  TraeChatSession,
} from '../types';

// 浏览器直接访问 Vite 开发服务器（如 http://localhost:5173）时没有 Tauri 运行时，
// 用于前端降级判断，避免调用 Tauri API 时崩溃白屏。
export const isTauri =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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
  license: {
    status: () => invoke<{ status: string; message: string }>('license_status'),
    activate: (code: string) =>
      invoke<{ status: string; message: string }>('license_activate', { code }),
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
    delete: (userId: string, deleteProfile: boolean, accountId?: string) =>
      invoke('account_delete', { userId, deleteProfile, accountId }),
    update: (userId: string, name?: string, jwt?: string, accountId?: string) =>
      invoke('account_update', { userId, name, jwt, accountId }),
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
  switchAccount: (userId: string) => invoke('trae_switch_account', { userId }),
  saveCurrentLogin: (userId: string) => invoke('trae_save_current_login', { userId }),
  listSnapshots: () => invoke<string[]>('list_snapshots'),
  resetDeviceIds: () => invoke('reset_device_ids'),
  resetDeviceCode: () => invoke('reset_device_code'),
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
  browserExtract: {
    start: (groupId?: string) => invoke('browser_extract_start', { groupId }),
    stop: () => invoke('browser_extract_stop'),
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
    // ---- M1：客户端登录态切换 ----
    authList: () => invoke<WbStoredAuth[]>('wb_auth_list'),
    authCurrent: () => invoke<WbAuthInfo | null>('wb_auth_current'),
    backupCurrentAuth: () => invoke<WbAuthInfo>('wb_auth_backup_current'),
    switchClient: (uid: string, opts?: { reload?: boolean; migrate?: 'auto' | 'ask' | 'off' }) =>
      invoke<WbSwitchResult>('wb_switch_account', { uid, opts }),
    // ---- M2：会话迁移 ----
    sessionList: (uid?: string) => invoke<WbSession[]>('wb_session_list', { uid }),
    sessionCopy: (sourceUid: string, sessionId: string, targetUid: string) =>
      invoke<{ sourceId: string; newId: string; status: string; failedFiles: number }>(
        'wb_session_copy',
        { sourceUid, sessionId, targetUid },
      ),
    sessionMigrateAll: (sourceUid: string, targetUid: string) =>
      invoke<WbSessionJob>('wb_session_migrate_all', { sourceUid, targetUid }),
    sessionJobStatus: () => invoke<WbSessionJob | null>('wb_session_job_status'),
    lineageNormalize: () => invoke<number>('wb_session_lineage_normalize'),
    // ---- M3：.wds 加密归档 ----
    sessionExport: (sessionId: string, password: string) =>
      invoke<string>('wb_session_export', { sessionId, password }),
    sessionImport: (wdsPath: string, password: string) =>
      invoke<string>('wb_session_import', { wdsPath, password }),
  },
  trae: {
    // ---- M4：Trae 对话解密导出 ----
    sessionList: () =>
      invoke<{ plain_db_path: string; sessions: TraeChatSession[] }>('trae_session_list'),
    exportMessages: (sessionId: string) =>
      invoke<string>('trae_session_export_messages', { sessionId }),
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

export interface DeviceCodeResetDoneEvent {
  success: boolean;
}

export interface ProfileDoneEvent {
  success: boolean;
  raw: string;
  action: 'backup' | 'restore';
}

export interface WbSwitchDoneEvent {
  success: boolean;
  uid: string;
  nickname?: string;
  sourceUid?: string;
  reloaded: boolean;
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
  onDeviceCodeResetProgress?: (line: string) => void;
  onDeviceCodeResetDone?: (e: DeviceCodeResetDoneEvent) => void;
  onProfileProgress?: (line: string) => void;
  onProfileDone?: (e: ProfileDoneEvent) => void;
  onWbSwitchDone?: (e: WbSwitchDoneEvent) => void;
  onWbSessionJob?: (e: WbSessionJob & { error?: string }) => void;
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
  if (handlers.onDeviceCodeResetProgress) {
    unsubs.push(
      await listen<string>('device-code-reset-progress', (e) =>
        handlers.onDeviceCodeResetProgress!(e.payload),
      ),
    );
  }
  if (handlers.onDeviceCodeResetDone) {
    unsubs.push(
      await listen<DeviceCodeResetDoneEvent>('device-code-reset-done', (e) =>
        handlers.onDeviceCodeResetDone!(e.payload),
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
  if (handlers.onWbSwitchDone) {
    unsubs.push(
      await listen<WbSwitchDoneEvent>('wb-switch-done', (e) =>
        handlers.onWbSwitchDone!(e.payload),
      ),
    );
  }
  if (handlers.onWbSessionJob) {
    unsubs.push(
      await listen<WbSessionJob & { error?: string }>('wb-session-job', (e) =>
        handlers.onWbSessionJob!(e.payload),
      ),
    );
  }
  return unsubs;
}
