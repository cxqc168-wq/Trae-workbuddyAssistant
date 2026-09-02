import { CheckCircle2, Info, AlertTriangle, XCircle, X } from 'lucide-react';
import { useAppStore, type ToastKind } from '../store';
import { cn } from '../lib/cn';

const iconMap: Record<ToastKind, typeof Info> = {
  info: Info,
  success: CheckCircle2,
  warn: AlertTriangle,
  error: XCircle,
};

const toneMap: Record<ToastKind, string> = {
  info: 'border-sky-300 bg-sky-50 text-sky-800 dark:border-sky-500/40 dark:bg-sky-500/10 dark:text-sky-200',
  success:
    'border-emerald-300 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-200',
  warn: 'border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-200',
  error:
    'border-rose-300 bg-rose-50 text-rose-800 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-200',
};

export default function Toaster() {
  const toasts = useAppStore((s) => s.toasts);
  const dismiss = useAppStore((s) => s.dismissToast);

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-80 flex-col gap-2">
      {toasts.map((t) => {
        const Icon = iconMap[t.kind];
        return (
          <div
            key={t.id}
            className={cn(
              'pointer-events-auto flex items-start gap-2 rounded-lg border px-3 py-2.5 text-sm shadow-lg animate-fade-in',
              toneMap[t.kind],
            )}
          >
            <Icon size={16} className="mt-0.5 shrink-0" />
            <span className="flex-1 break-words">{t.msg}</span>
            <button
              onClick={() => dismiss(t.id)}
              className="shrink-0 opacity-60 hover:opacity-100"
              aria-label="关闭"
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
