import { type ReactNode, useEffect } from 'react';
import { X } from 'lucide-react';
import { cn } from '../lib/cn';

type Tone =
  | 'slate'
  | 'brand'
  | 'green'
  | 'amber'
  | 'red'
  | 'blue'
  | 'violet';

const toneMap: Record<Tone, string> = {
  slate: 'bg-slate-100 text-slate-600 dark:bg-zinc-800 dark:text-zinc-300',
  brand: 'bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-200',
  green: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300',
  amber: 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300',
  red: 'bg-rose-100 text-rose-700 dark:bg-rose-500/15 dark:text-rose-300',
  blue: 'bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300',
  violet: 'bg-violet-100 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300',
};

const accentMap: Record<Tone, string> = {
  slate: 'bg-slate-400',
  brand: 'bg-zinc-500',
  green: 'bg-emerald-500',
  amber: 'bg-amber-500',
  red: 'bg-rose-500',
  blue: 'bg-sky-500',
  violet: 'bg-violet-500',
};

const valueToneMap: Record<Tone, string> = {
  slate: 'text-slate-800 dark:text-zinc-100',
  brand: 'text-zinc-700 dark:text-zinc-200',
  green: 'text-emerald-600 dark:text-emerald-400',
  amber: 'text-amber-600 dark:text-amber-400',
  red: 'text-rose-600 dark:text-rose-400',
  blue: 'text-sky-600 dark:text-sky-400',
  violet: 'text-violet-600 dark:text-violet-400',
};

export function Badge({
  children,
  tone = 'slate',
  className,
}: {
  children: ReactNode;
  tone?: Tone;
  className?: string;
}) {
  return <span className={cn('chip', toneMap[tone], className)}>{children}</span>;
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        'inline-block h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent',
        className,
      )}
      role="status"
      aria-label="loading"
    />
  );
}

export function Progress({
  value,
  max = 100,
  className,
}: {
  value: number;
  max?: number;
  className?: string;
}) {
  const pct = max <= 0 ? 0 : Math.min(100, Math.round((value / max) * 100));
  return (
    <div className={cn('h-2 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-zinc-800', className)}>
      <div
        className="h-full rounded-full bg-zinc-800 dark:bg-zinc-200 transition-all"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function Modal({
  open,
  onClose,
  title,
  children,
  footer,
  size,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  size?: 'lg' | 'xl';
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', onKey);
      document.body.style.overflow = '';
    };
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" onClick={onClose} />
      <div className={`card relative z-10 w-full ${size === 'xl' ? 'max-w-4xl' : size === 'lg' ? 'max-w-2xl' : 'max-w-lg'} animate-fade-in p-5 shadow-xl`} role="dialog" aria-modal="true">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-base font-semibold text-slate-800 dark:text-zinc-100">{title}</h3>
          <button className="btn-ghost h-8 w-8 !p-0" onClick={onClose} aria-label="关闭">
            <X size={16} />
          </button>
        </div>
        <div className="text-sm text-slate-600 dark:text-zinc-300">{children}</div>
        {footer && <div className="mt-5 flex justify-end gap-2">{footer}</div>}
      </div>
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  hint,
}: {
  icon?: ReactNode;
  title: string;
  hint?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-slate-300 py-12 text-center dark:border-zinc-700">
      {icon && (
        <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-slate-100 text-slate-400 dark:bg-zinc-800">
          {icon}
        </div>
      )}
      <p className="text-sm font-medium text-slate-600 dark:text-zinc-300">{title}</p>
      {hint && <p className="mt-1 max-w-sm text-xs text-slate-400">{hint}</p>}
    </div>
  );
}

export function StatCard({
  label,
  value,
  hint,
  tone = 'slate',
}: {
  label: string;
  value: ReactNode;
  hint?: string;
  tone?: Tone;
}) {
  return (
    <div className="card relative overflow-hidden p-4">
      <div className={cn('absolute inset-x-0 top-0 h-0.5', accentMap[tone])} />
      <div className="text-xs font-medium text-slate-500 dark:text-zinc-400">{label}</div>
      <div className={cn('mt-1 text-2xl font-semibold tracking-tight tabular-nums', valueToneMap[tone])}>
        {value}
      </div>
      {hint && <div className="mt-1 text-xs text-slate-400">{hint}</div>}
    </div>
  );
}
