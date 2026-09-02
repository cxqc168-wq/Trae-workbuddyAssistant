// WorkBuddy 三页面（账号 / 签到 / 积分）共用工具与组件
import { AlertTriangle, CheckCircle2, HelpCircle, XCircle } from 'lucide-react';
import { Badge } from '../components/ui';
import type { WorkBuddyAccountMeta } from '../types';

export const MS_PER_DAY = 24 * 3600 * 1000;

export function displayName(a: WorkBuddyAccountMeta): string {
  return a.nickname || a.email || a.uid || '未命名账号';
}

export function formatNum(n: number | null | undefined): string {
  if (n == null || Number.isNaN(n)) return '-';
  return n.toLocaleString('zh-CN', { maximumFractionDigits: 2 });
}

export function formatTime(ms: number | null | undefined): string {
  if (ms == null) return '-';
  return new Date(ms).toLocaleString('zh-CN');
}

// token 状态：需重登 > 已过期/临期(<24h)/有效 > 未知
export function TokenStatusBadge({ account }: { account: WorkBuddyAccountMeta }) {
  if (account.needsRelogin) {
    return (
      <span title={account.needsReloginReason ?? '需要重新登录'}>
        <Badge tone="red">
          <AlertTriangle size={12} /> 需重登
        </Badge>
      </span>
    );
  }
  if (account.expiresAt == null) {
    return (
      <Badge tone="slate">
        <HelpCircle size={12} /> 未知
      </Badge>
    );
  }
  const remain = account.expiresAt - Date.now();
  if (remain <= 0) {
    return (
      <Badge tone="red">
        <XCircle size={12} /> 已过期
      </Badge>
    );
  }
  if (remain < MS_PER_DAY) {
    return (
      <Badge tone="amber">
        <AlertTriangle size={12} /> 临期
      </Badge>
    );
  }
  return (
    <Badge tone="green">
      <CheckCircle2 size={12} /> 有效
    </Badge>
  );
}
