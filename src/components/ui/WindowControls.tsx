import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "../../lib/format";

const win = () => getCurrentWindow();

/** Minimise / maximise / close for ENDownloader's frameless window. */
export function WindowControls({ dark = false }: { dark?: boolean }) {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    win().isMaximized().then(setMaximized, () => {});
    const un = win().onResized(() => win().isMaximized().then(setMaximized, () => {}));
    return () => {
      un.then((f) => f());
    };
  }, []);
  const btn = cn(
    "flex h-6 w-6 items-center justify-center rounded-full transition-colors",
    dark ? "bg-vfg/10 text-vfg/80 hover:bg-vfg/20 hover:text-vfg" : "bg-raised text-muted hover:bg-line hover:text-fg",
  );
  return (
    <div className="ml-1 flex items-center gap-2 pl-1" onMouseDown={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
      <button className={btn} aria-label="Minimize" title="Minimize" onClick={() => win().minimize()}>
        <Minus className="h-3.5 w-3.5" />
      </button>
      <button className={btn} aria-label={maximized ? "Restore" : "Maximize"} title={maximized ? "Restore" : "Maximize"} onClick={() => win().toggleMaximize()}>
        {maximized ? <Copy className="h-3 w-3 -scale-x-100" /> : <Square className="h-3 w-3" />}
      </button>
      <button className={cn(btn, "hover:bg-danger hover:text-white")} aria-label="Close" title="Close" onClick={() => win().close()}>
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

/**
 * Props for a bar that should move the window: drag on empty space,
 * double-click to maximise. Clicks on buttons, inputs and menus are ignored.
 */
export const windowDragProps = {
  onMouseDown: (e: React.MouseEvent) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button,input,select,textarea,a,[role=menu],[data-no-drag]")) return;
    if (e.detail === 2) win().toggleMaximize();
    else win().startDragging();
  },
};
