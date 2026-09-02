import { useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X } from 'lucide-react';
import { useAppStore } from '../store';
import { Modal } from './ui';

const win = getCurrentWindow();

export default function TitleBar() {
  const [showCloseConfirm, setShowCloseConfirm] = useState(false);
  const proxyRunning = useAppStore((s) => s.proxy.running);

  return (
    <>
      <div
        className="flex h-9 shrink-0 items-center justify-between border-b border-slate-200 bg-slate-50 pl-3 pr-1 dark:border-zinc-800 dark:bg-zinc-950"
      >
        <div data-tauri-drag-region className="flex flex-1 items-center gap-2">
          <span className="flex h-6 w-6 items-center justify-center rounded-lg bg-gradient-to-br from-brand-500 to-brand-700 text-[10px] font-bold tracking-tight text-white shadow-sm dark:from-brand-400 dark:to-brand-600">
            TW
          </span>
          <span className="text-sm font-semibold text-slate-700 dark:text-zinc-200">
            Trae Work 助手
          </span>
        </div>
        <div className="flex items-center">
          {/* onMouseDown 阻止冒泡：否则事件冒泡到外层 data-tauri-drag-region，Tauri 会启动窗口拖拽而吞掉 click，导致最小/最大化/关闭无响应 */}
          <button
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => void win.hide()}
            className="flex h-8 w-10 items-center justify-center text-slate-500 transition hover:bg-slate-200/70 active:scale-90 dark:text-zinc-400 dark:hover:bg-zinc-800"
            aria-label="最小化"
            title="最小化到托盘"
          >
            <Minus size={15} />
          </button>
          <button
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => void win.toggleMaximize()}
            className="flex h-8 w-10 items-center justify-center text-slate-500 transition hover:bg-slate-200/70 active:scale-90 dark:text-zinc-400 dark:hover:bg-zinc-800"
            aria-label="最大化"
          >
            <Square size={13} />
          </button>
          <button
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => setShowCloseConfirm(true)}
            className="flex h-8 w-10 items-center justify-center text-slate-500 transition hover:bg-rose-500 hover:text-white active:scale-90"
            aria-label="关闭"
          >
            <X size={15} />
          </button>
        </div>
      </div>

      <Modal
        open={showCloseConfirm}
        onClose={() => setShowCloseConfirm(false)}
        title="确认退出应用"
        footer={
          <>
            <button className="btn-outline" onClick={() => setShowCloseConfirm(false)}>
              取消
            </button>
            <button className="btn-danger" onClick={() => void win.close()}>
              确认退出
            </button>
          </>
        }
      >
        {proxyRunning ? (
          <p>
            检测到代理正在运行，退出时将自动关闭代理并还原系统代理设置。
            <br />
            确认退出 Trae Work 助手？
          </p>
        ) : (
          <p>确认退出 Trae Work 助手？</p>
        )}
      </Modal>
    </>
  );
}
