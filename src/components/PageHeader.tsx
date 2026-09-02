import { type ReactNode } from 'react';

export default function PageHeader({
  title,
  desc,
  actions,
}: {
  title: string;
  desc?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="mb-5 flex items-end justify-between gap-4">
      <div className="flex items-center gap-3">
        <div className="h-7 w-1 rounded-full bg-zinc-800 dark:bg-zinc-200" />
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-slate-800 dark:text-zinc-100">{title}</h1>
          {desc && <p className="mt-1 text-sm text-slate-500 dark:text-zinc-400">{desc}</p>}
        </div>
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}
