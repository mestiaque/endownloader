import type { Task } from "../lib/types";

const UNFINISHED = new Set(["queued", "starting", "active", "paused"]);

/**
 * The current batch: everything not finished yet, plus what finished since the oldest
 * unfinished download was added (so "2 of 5" counts the files of this round).
 */
export function batchSummary(tasks: Task[]) {
  const open = tasks.filter((t) => UNFINISHED.has(t.status));
  const since = open.length ? Math.min(...open.map((t) => t.createdAt)) : Infinity;
  const finished = tasks.filter((t) => t.status === "completed" && (t.finishedAt ?? 0) >= since);
  const batch = [...open, ...finished];
  const total = batch.reduce((n, t) => n + (t.total ?? 0), 0);
  const done = batch.reduce((n, t) => n + (t.status === "completed" ? t.total ?? t.done : t.done), 0);
  const speed = open.reduce((n, t) => n + (t.status === "active" ? t.speed : 0), 0);
  const running = open.filter((t) => t.status === "active" || t.status === "starting").length;
  const unknownSize = open.some((t) => t.total == null);
  return {
    count: batch.length,
    finished: finished.length,
    open: open.length,
    running,
    paused: open.length > 0 && open.every((t) => t.status === "paused"),
    total,
    done,
    speed,
    percent: total ? Math.min(100, (done / total) * 100) : 0,
    // Remaining time for the sizes known so far; queued files often report their size only
    // when they start, so the UI shows "≥" while some sizes are still unknown.
    eta: speed > 0 && total > done ? (total - done) / speed : null,
    unknownSize,
    current: open.find((t) => t.status === "active") ?? open[0] ?? null,
  };
}

