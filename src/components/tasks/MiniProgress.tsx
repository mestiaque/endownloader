import { ChevronUp, Download, Pause } from "lucide-react";
import { useMemo } from "react";
import { formatBytes, formatEta, formatSpeed } from "../../lib/format";
import { useTasks } from "../../store/tasks";
import { useUi } from "../../store/ui";
import { batchSummary } from "../../engines/batch";

/** The minimised download list: one progress bar for the whole batch. */
export function MiniProgress() {
  const tasks = useTasks((s) => s.tasks);
  const s = useMemo(() => batchSummary(tasks), [tasks]);
  const expand = () => useUi.getState().setListCollapsed(false);
  const completedAll = tasks.filter((t) => t.status === "completed").length;
  const name = s.current ? s.current.title || s.current.filename || s.current.url : "";

  return (
    <div className="flex h-12 shrink-0 items-center gap-3 border-t border-line bg-panel px-3 text-[12px]">
      <span className={s.open ? "text-accent" : "text-faint"}>{s.paused ? <Pause className="h-4 w-4" /> : <Download className="h-4 w-4" />}</span>
      {s.open ? (
        <>
          <div className="w-[130px] shrink-0">
            <div className="font-semibold tabular-nums">
              {s.paused ? "Paused" : "Downloading"} {Math.min(s.finished + 1, s.count)} of {s.count}
            </div>
            <div className="truncate text-[11px] text-faint" title={name}>{name}</div>
          </div>
          <div className="min-w-0 flex-1">
            <div className="h-2 overflow-hidden rounded-full bg-line">
              <div
                className={s.total ? "h-full rounded-full bg-accent transition-[width] duration-300" : "h-full w-1/3 animate-pulse rounded-full bg-accent"}
                style={s.total ? { width: `${s.percent}%` } : undefined}
              />
            </div>
          </div>
          <span className="w-[54px] shrink-0 text-right font-semibold tabular-nums">{s.total ? `${Math.floor(s.percent)}%` : "—"}</span>
          <span className="w-[150px] shrink-0 text-right tabular-nums text-muted">
            {formatBytes(s.done)}{s.total ? ` of ${formatBytes(s.total)}` : ""}
          </span>
          <span className="w-[86px] shrink-0 text-right tabular-nums">{formatSpeed(s.speed)}</span>
          <span className="w-[78px] shrink-0 text-right tabular-nums text-muted" title={s.unknownSize ? "At least this long: some queued files have not reported their size yet" : "Time left"}>
            {s.eta != null && s.unknownSize ? "≥ " : ""}{formatEta(s.eta)}
          </span>
        </>
      ) : (
        <span className="flex-1 text-muted">No downloads running{completedAll ? ` · ${completedAll} completed` : ""}</span>
      )}
      <button
        className="flex h-8 shrink-0 items-center gap-1 rounded-md px-2 text-muted hover:bg-raised hover:text-fg"
        onClick={expand}
        title="Show the download list"
      >
        <ChevronUp className="h-4 w-4" />
        List
      </button>
    </div>
  );
}
