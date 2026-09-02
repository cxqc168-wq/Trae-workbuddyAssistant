import { useEffect, useRef, useState } from 'react';
import { Check, Copy, ExternalLink, Github, MessageCircle, QrCode } from 'lucide-react';
import { open } from '@tauri-apps/plugin-shell';
import { useAppStore } from '../store';

// ===== 作者信息（防篡改）=====
// 常量以 XOR 混淆存储，运行时解码并做 SHA-256 完整性自检；
// 若被修改，卡片会弹出校验失败警告。请尊重开源作者，勿移除或篡改本区块。
const _K = 'Gp2#x9Kv7QzLm4Tw';
const _N = 'oe6zxcuzGxlbNAk=';
const _U = 'LwRGUwsDZFlQOA4kGFZ6FCgdHUAASChHAWlXOxw=';
const _I = 'JAhDQEkPc1tAIA==';
const _Q = 'dUABFkgMfUYO';
const _S = '35111522ba8beb8fa8a09b680ffdd3f70980b0cb2b7bdfe8091da25fb295fb9a';

function _d(s: string): string {
  const raw = atob(s);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) {
    bytes[i] = raw.charCodeAt(i) ^ _K.charCodeAt(i % _K.length);
  }
  return new TextDecoder().decode(bytes);
}

async function _sig(): Promise<string> {
  const data = [AUTHOR_NAME, AUTHOR_GITHUB, AUTHOR_GITHUB_ID, AUTHOR_QQ].join('|');
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(data));
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

const AUTHOR_NAME = _d(_N);
const AUTHOR_GITHUB = _d(_U);
const AUTHOR_GITHUB_ID = _d(_I);
const AUTHOR_QQ = _d(_Q);

// 收款码图片：替换 src/assets/reward-qr.png 即可更新
import rewardQr from '../assets/reward-qr.png';
const REWARD_QR_SRC: string | null = rewardQr;

function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(text);
  return new Promise((resolve, reject) => {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand('copy') ? resolve() : reject(new Error('copy failed'));
    } finally {
      document.body.removeChild(ta);
    }
  });
}

export default function AuthorCard({ onClose }: { onClose: () => void }) {
  const pushToast = useAppStore((s) => s.pushToast);
  const rootRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [onClose]);

  // 完整性自检：作者信息被篡改时警告
  useEffect(() => {
    void _sig().then((h) => {
      if (h !== _S) {
        pushToast('error', '作者信息校验失败：检测到源码被篡改，请尊重开源作者');
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const openGithub = async () => {
    try {
      await open(AUTHOR_GITHUB);
    } catch (e) {
      pushToast('error', `打开 GitHub 失败：${String(e)}`);
    }
  };

  const copyQQ = async () => {
    try {
      await copyText(AUTHOR_QQ);
      setCopied(true);
      pushToast('success', `QQ 号已复制：${AUTHOR_QQ}`);
      setTimeout(() => setCopied(false), 1600);
    } catch (e) {
      pushToast('error', `复制失败，请手动选择：${AUTHOR_QQ}`);
    }
  };

  return (
    <div
      ref={rootRef}
      className="absolute bottom-full left-3 z-50 mb-2 w-72 origin-bottom-left animate-pop-in rounded-2xl border border-slate-200 bg-white shadow-soft-lg dark:border-zinc-800 dark:bg-zinc-900"
      role="dialog"
      aria-label="作者详情"
    >
      {/* 头部：头像 + 作者名 */}
      <div className="flex items-center gap-3 border-b border-slate-100 bg-gradient-to-br from-slate-50 to-white p-4 dark:border-zinc-800 dark:from-zinc-900 dark:to-zinc-900">
        <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-zinc-800 to-zinc-600 text-lg font-bold text-white shadow-inner dark:from-zinc-100 dark:to-zinc-400 dark:text-zinc-900">
          极
        </div>
        <div className="min-w-0">
          <p className="truncate text-sm font-bold text-slate-900 dark:text-zinc-100">{AUTHOR_NAME}</p>
          <p className="text-xs text-slate-400 dark:text-zinc-500">本项目作者 · Developer</p>
        </div>
      </div>

      {/* 联系方式 */}
      <div className="space-y-1 p-2">
        <button
          onClick={openGithub}
          className="group flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-medium text-slate-700 transition hover:bg-slate-100 active:scale-[0.98] dark:text-zinc-300 dark:hover:bg-zinc-800"
        >
          <Github size={16} className="shrink-0 text-slate-500 dark:text-zinc-400" />
          <span className="flex-1 truncate text-left">{AUTHOR_GITHUB_ID}</span>
          <ExternalLink
            size={13}
            className="shrink-0 text-slate-400 opacity-0 transition group-hover:opacity-100 dark:text-zinc-500"
          />
        </button>
        <button
          onClick={copyQQ}
          className="group flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-medium text-slate-700 transition hover:bg-slate-100 active:scale-[0.98] dark:text-zinc-300 dark:hover:bg-zinc-800"
        >
          <MessageCircle size={16} className="shrink-0 text-slate-500 dark:text-zinc-400" />
          <span className="flex-1 truncate text-left">QQ：{AUTHOR_QQ}</span>
          {copied ? (
            <Check size={13} className="shrink-0 text-emerald-500" />
          ) : (
            <Copy
              size={13}
              className="shrink-0 text-slate-400 opacity-0 transition group-hover:opacity-100 dark:text-zinc-500"
            />
          )}
        </button>
      </div>

      {/* 奖励作者：收款码 */}
      <div className="border-t border-slate-100 p-4 dark:border-zinc-800">
        <p className="mb-2.5 text-xs font-semibold tracking-wide text-slate-400 dark:text-zinc-500">
          奖励作者
        </p>
        {REWARD_QR_SRC ? (
          <img
            src={REWARD_QR_SRC}
            alt="收款码"
            className="mx-auto h-40 w-40 rounded-xl border border-slate-200 object-contain dark:border-zinc-800"
          />
        ) : (
          <div className="mx-auto flex h-40 w-40 flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-slate-200 bg-slate-50 text-slate-300 dark:border-zinc-700 dark:bg-zinc-950 dark:text-zinc-600">
            <QrCode size={32} />
            <p className="text-[11px]">收款码占位</p>
            <p className="px-3 text-center text-[10px] leading-relaxed">
              替换 src/assets/reward-qr.png
            </p>
          </div>
        )}
        <p className="mt-2.5 text-center text-[11px] text-slate-400 dark:text-zinc-500">
          如果这个工具帮到了你，可以请作者喝杯咖啡
        </p>
      </div>
    </div>
  );
}
