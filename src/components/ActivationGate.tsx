import { useState } from 'react';
import { api } from '../lib/tauri';
import wechatQr from '../assets/wechat-qr.jpg';

interface Props {
  onSuccess: () => void;
}

/**
 * 授权激活门：license-guard 防护校验未通过时替换整个应用界面。
 * 关注公众号获取口令 -> 输入口令 -> 后端调用验证服务器换取凭证 -> 成功后放行主应用。
 */
export default function ActivationGate({ onSuccess }: Props) {
  const [code, setCode] = useState('');
  const [message, setMessage] = useState('本软件需要激活后使用，请输入激活口令');
  const [messageKind, setMessageKind] = useState<'info' | 'error'>('info');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const c = code.trim();
    if (!c) {
      setMessageKind('error');
      setMessage('请输入激活口令');
      return;
    }
    if (busy) return;
    setBusy(true);
    try {
      const r = await api.license.activate(c);
      if (r.status === 'ok') {
        onSuccess();
      } else {
        setMessageKind('error');
        setMessage(r.message || '激活失败');
      }
    } catch (e) {
      setMessageKind('error');
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full w-full items-center justify-center bg-slate-100 dark:bg-zinc-950">
      <div className="flex w-[640px] max-w-[92vw] flex-col gap-6 rounded-xl border border-slate-200 bg-white p-7 shadow-lg dark:border-zinc-800 dark:bg-zinc-900 sm:flex-row">
        {/* 左：公众号引流 */}
        <div className="flex flex-col items-center gap-3 sm:w-[220px]">
          <img
            src={wechatQr}
            alt="公众号二维码"
            className="h-[200px] w-[200px] rounded-lg border border-slate-200 object-cover dark:border-zinc-700"
          />
          <div className="flex flex-col items-center gap-1">
            <p className="text-sm font-medium text-slate-800 dark:text-zinc-100">
              关注公众号【极泊说】
            </p>
            <p className="text-xs text-slate-500 dark:text-zinc-400">获取软件激活口令</p>
          </div>
        </div>

        {/* 右：激活表单 */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="mb-5 flex flex-col items-center gap-2 sm:items-start">
            <div className="flex h-11 w-11 items-center justify-center rounded-full bg-sky-50 dark:bg-sky-950/60">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
                strokeLinejoin="round"
                className="h-6 w-6 text-sky-600 dark:text-sky-400"
              >
                <path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6l7-3z" />
                <path d="M9 12l2 2 4-4" />
              </svg>
            </div>
            <h1 className="text-lg font-semibold text-slate-800 dark:text-zinc-100">软件激活</h1>
            <p className="text-xs leading-5 text-slate-500 dark:text-zinc-400">
              授权与本机绑定，激活后 7 天内离线可用
              <br />
              到期后重新输入口令即可续期
            </p>
          </div>

          <div className="flex flex-col gap-3">
            <input
              type="text"
              value={code}
              autoFocus
              spellCheck={false}
              disabled={busy}
              placeholder="请输入激活口令"
              onChange={(e) => setCode(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void submit();
              }}
              className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-800 outline-none transition focus:border-sky-500 focus:ring-2 focus:ring-sky-500/20 disabled:opacity-60 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100 dark:focus:border-sky-500"
            />
            <button
              type="button"
              disabled={busy}
              onClick={() => void submit()}
              className="w-full rounded-lg bg-sky-600 px-3 py-2 text-sm font-medium text-white transition hover:bg-sky-500 active:scale-[0.99] disabled:cursor-not-allowed disabled:opacity-60"
            >
              {busy ? '正在激活…' : '激活'}
            </button>
            <p
              className={`min-h-[1.25rem] text-center text-xs leading-5 sm:text-left ${
                messageKind === 'error'
                  ? 'text-red-500 dark:text-red-400'
                  : 'text-slate-500 dark:text-zinc-400'
              }`}
            >
              {message}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
