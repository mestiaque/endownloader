import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownToLine, Copy, ExternalLink, FolderOpen, Pause, Play, RotateCcw, Trash2, X } from "lucide-react";
import { memo, useEffect, useRef } from "react";
import { canCancel, canPause, canResume, canRetry, displayName, etaSeconds, percent } from "../../engines/taskView";
import { api } from "../../lib/api";
import { cn, formatBytes, formatEta, formatSpeed, hostOf } from "../../lib/format";
import type { Task } from "../../lib/types";
import { removeTasks, runAction, targetTasks, useTasks, useVisibleTasks } from "../../store/tasks";
import { toast } from "../../store/ui";
import { ContextMenu, type MenuEntry } from "../ui/menus";
import { CategoryIcon } from "./categoryIcon";

const ROW_H = 52;

const STATUS_TEXT: Record<Task["status"], string> = {
  queued: "Queued",
  starting: "Starting…",
  active: "Downloading",
  paused: "Paused",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};

function filePath(t: Task): string {
  const sep = t.folder.includes("\\") ? "\\" : "/";
  return t.filename ? `${t.folder.replace(/[\\/]$/, "")}${sep}${t.filename}` : t.folder;
}

function ProgressCell({ t }: { t: Task }) {
  const pct = percent(t);
  const running = t.status === "active" || t.status === "starting";
  const color = t.status === "failed" ? "bg-danger" : t.status === "completed" ? "bg-success" : t.status === "paused" || t.status === "queued" ? "bg-faint" : "bg-accent";
  return (
    <div className="w-full">
      <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-line">
        {pct == null && running ? <div className="progress-indeterminate" /> : <div className={cn("h-full rounded-full transition-[width] duration-300", color)} style={{ width: `${pct ?? 0}%` }} />}
      </div>
      <div className={cn("mt-1 flex justify-between text-[11px] tabular-nums", t.status === "failed" ? "text-danger" : "text-muted")}>
        <span>{STATUS_TEXT[t.status]}</span>
        <span>{pct != null && t.status !== "completed" ? `${pct.toFixed(1)}%` : ""}</span>
      </div>
    </div>
  );
}

function menuFor(t: Task): MenuEntry[] {
  const sel = useTasks.getState().selection;
  const targets = sel.has(t.id) ? targetTasks() : [t];
  const many = targets.length > 1;
  return [
    { label: many ? `Resume ${targets.length}` : "Resume", icon: <Play className="h-3.5 w-3.5" />, hidden: !targets.some(canResume), onSelect: () => runAction(targets.filter(canResume), "resume") },
    { label: many ? `Pause ${targets.length}` : "Pause", icon: <Pause className="h-3.5 w-3.5" />, hidden: !targets.some(canPause), onSelect: () => runAction(targets.filter(canPause), "pause") },
    { label: "Download again", icon: <RotateCcw className="h-3.5 w-3.5" />, hidden: !targets.some(canRetry), onSelect: () => runAction(targets.filter(canRetry), "retry") },
    { label: "Cancel", icon: <X className="h-3.5 w-3.5" />, hidden: !targets.some(canCancel), onSelect: () => runAction(targets.filter(canCancel), "cancel") },
    { separator: true },
    { label: "Open file", icon: <ArrowDownToLine className="h-3.5 w-3.5" />, hidden: many || t.status !== "completed", onSelect: () => api.openPath(filePath(t)).catch(() => toast({ kind: "error", title: "Could not open the file" })) },
    { label: "Show in folder", icon: <FolderOpen className="h-3.5 w-3.5" />, hidden: many, onSelect: () => api.revealPath(filePath(t)).catch(() => toast({ kind: "error", title: "The folder does not exist yet" })) },
    { label: "Open source link", icon: <ExternalLink className="h-3.5 w-3.5" />, hidden: many, onSelect: () => void api.openUrl(t.url) },
    {
      label: many ? "Copy links" : "Copy link",
      icon: <Copy className="h-3.5 w-3.5" />,
      onSelect: () => navigator.clipboard.writeText(targets.map((x) => x.url).join("\n")).then(() => toast({ kind: "success", title: "Copied" }), () => {}),
    },
    { separator: true },
    { label: many ? `Remove ${targets.length} from list` : "Remove from list", icon: <Trash2 className="h-3.5 w-3.5" />, shortcut: "Del", onSelect: () => removeTasks(targets) },
    { label: "Delete with files…", danger: true, shortcut: "Shift+Del", onSelect: () => removeTasks(targets, true) },
  ];
}

/** Media tasks keep the video thumbnail from the analysis. */
function thumbOf(t: Task): string | null {
  const o = t.options as { thumbnail?: unknown } | null;
  return o && typeof o.thumbnail === "string" && o.thumbnail.startsWith("https://") ? o.thumbnail : null;
}

const TaskRow = memo(function TaskRow({ t, selected, focused, onClick }: { t: Task; selected: boolean; focused: boolean; onClick: (e: React.MouseEvent, t: Task) => void }) {
  const eta = etaSeconds(t);
  const running = t.status === "active" || t.status === "starting";
  return (
    <ContextMenu entries={() => menuFor(t)} onOpen={() => !useTasks.getState().selection.has(t.id) && useTasks.getState().setSelection([t.id])}>
      <div
        role="row"
        aria-selected={selected}
        onMouseDown={(e) => e.button === 0 && onClick(e, t)}
        onDoubleClick={() => t.status === "completed" && api.openPath(filePath(t)).catch(() => toast({ kind: "error", title: "Could not open the file" }))}
        className={cn(
          "group grid h-full cursor-default grid-cols-[minmax(0,1fr)_180px_130px_90px_70px_72px] items-center gap-3 border-b border-line/50 px-3",
          selected ? "bg-accent/10" : "hover:bg-raised/50",
          focused && "shadow-[inset_2px_0_0_rgb(var(--accent))]",
        )}
      >
        <div className="flex min-w-0 items-center gap-3">
          {thumbOf(t) ? (
            <img src={thumbOf(t)!} alt="" loading="lazy" referrerPolicy="no-referrer" className="h-8 w-[52px] shrink-0 rounded-md border border-line object-cover" />
          ) : (
            <div className={cn("flex h-8 w-8 shrink-0 items-center justify-center rounded-lg", t.status === "failed" ? "bg-danger/10 text-danger" : t.status === "completed" ? "bg-success/10 text-success" : "bg-accent/10 text-accent")}>
              <CategoryIcon id={t.category} />
            </div>
          )}
          <div className="min-w-0">
            <div className="truncate text-[13px] font-medium" title={displayName(t)}>
              {displayName(t)}
            </div>
            <div className={cn("truncate text-[11px]", t.error ? "text-danger" : "text-faint")} title={t.error ?? t.url}>
              {t.error ?? hostOf(t.url)}
            </div>
          </div>
        </div>
        <ProgressCell t={t} />
        <div className="text-right text-[12px] tabular-nums text-muted">
          {t.total ? (t.status === "completed" ? formatBytes(t.total) : `${formatBytes(t.done)} / ${formatBytes(t.total)}`) : t.done > 0 ? formatBytes(t.done) : "—"}
        </div>
        <div className="text-right text-[12px] tabular-nums text-muted">{running ? formatSpeed(t.speed) : "—"}</div>
        <div className="text-right text-[12px] tabular-nums text-muted">{running ? formatEta(eta) : "—"}</div>
        <div className="flex justify-end gap-0.5 opacity-0 transition-opacity group-hover:opacity-100" onMouseDown={(e) => e.stopPropagation()}>
          {canPause(t) ? (
            <button className="rounded p-1 text-muted hover:bg-line hover:text-fg" aria-label="Pause" title="Pause" onClick={() => runAction([t], "pause")}>
              <Pause className="h-3.5 w-3.5" />
            </button>
          ) : canResume(t) ? (
            <button className="rounded p-1 text-muted hover:bg-line hover:text-fg" aria-label="Resume" title="Resume" onClick={() => runAction([t], "resume")}>
              <Play className="h-3.5 w-3.5" />
            </button>
          ) : canRetry(t) ? (
            <button className="rounded p-1 text-muted hover:bg-line hover:text-fg" aria-label="Download again" title="Download again" onClick={() => runAction([t], "retry")}>
              <RotateCcw className="h-3.5 w-3.5" />
            </button>
          ) : null}
          <button className="rounded p-1 text-muted hover:bg-line hover:text-fg" aria-label="Show in folder" title="Show in folder" onClick={() => api.revealPath(filePath(t)).catch(() => toast({ kind: "error", title: "The folder does not exist yet" }))}>
            <FolderOpen className="h-3.5 w-3.5" />
          </button>
          <button className="rounded p-1 text-muted hover:bg-line hover:text-danger" aria-label="Remove" title="Remove from list" onClick={() => removeTasks([t])}>
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </ContextMenu>
  );
});

function Empty() {
  const view = useTasks((s) => s.view);
  const total = useTasks((s) => s.tasks.length);
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent/10 text-accent">
        <ArrowDownToLine className="h-8 w-8" />
      </div>
      {total === 0 ? (
        <>
          <div className="text-[15px] font-semibold">No downloads yet</div>
          <div className="max-w-sm text-[13px] text-muted">Paste one or more links in the box above, or click Paste &amp; Analyze. Files, videos and whole playlists are all welcome.</div>
        </>
      ) : (
        <div className="text-[13px] text-muted">Nothing here{view !== "all" ? " in this view" : ""}.</div>
      )}
    </div>
  );
}

export function TaskList() {
  const visible = useVisibleTasks();
  const selection = useTasks((s) => s.selection);
  const focused = useTasks((s) => s.focused);
  const parentRef = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({ count: visible.length, getScrollElement: () => parentRef.current, estimateSize: () => ROW_H, overscan: 12 });

  // Keep the keyboard-focused row in view.
  useEffect(() => {
    if (focused == null) return;
    const i = visible.findIndex((t) => t.id === focused);
    if (i >= 0) v.scrollToIndex(i, { align: "auto" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused]);

  const onClick = (e: React.MouseEvent, t: Task) => {
    const mode = e.shiftKey ? "range" : e.ctrlKey || e.metaKey ? "toggle" : "single";
    useTasks.getState().select(t.id, mode, visible);
  };

  if (!visible.length) return <Empty />;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="grid h-8 shrink-0 grid-cols-[minmax(0,1fr)_180px_130px_90px_70px_72px] items-center gap-3 border-b border-line px-3 text-[11px] font-semibold uppercase tracking-wider text-faint">
        <span>Name</span>
        <span>Progress</span>
        <span className="text-right">Size</span>
        <span className="text-right">Speed</span>
        <span className="text-right">ETA</span>
        <span />
      </div>
      <div
        ref={parentRef}
        role="grid"
        aria-label="Downloads"
        aria-multiselectable
        className="min-h-0 flex-1 overflow-y-auto"
        onMouseDown={(e) => e.target === e.currentTarget && useTasks.getState().clearSelection()}
      >
        <div style={{ height: v.getTotalSize(), position: "relative" }}>
          {v.getVirtualItems().map((row) => {
            const t = visible[row.index];
            return (
              <div key={t.id} style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW_H, transform: `translateY(${row.start}px)` }}>
                <TaskRow t={t} selected={selection.has(t.id)} focused={focused === t.id} onClick={onClick} />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
