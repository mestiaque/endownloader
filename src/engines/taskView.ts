/** Pure helpers for the task list: views, counts, progress and ETA. Unit-tested. */
import type { Task, TaskStatus } from "../lib/types";

export type ViewId = "all" | "active" | "queued" | "completed" | "failed" | `cat:${string}`;

const STATUS_VIEWS: Record<Exclude<ViewId, `cat:${string}`>, TaskStatus[] | null> = {
  all: null,
  active: ["starting", "active"],
  queued: ["queued", "paused"],
  completed: ["completed"],
  failed: ["failed"],
};

export function inView(t: Task, view: ViewId): boolean {
  if (view.startsWith("cat:")) return t.category === view.slice(4);
  const st = STATUS_VIEWS[view as keyof typeof STATUS_VIEWS];
  return st === null || st === undefined || st.includes(t.status);
}

export function matchesSearch(t: Task, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase();
  return (t.title ?? "").toLowerCase().includes(needle) || (t.filename ?? "").toLowerCase().includes(needle) || t.url.toLowerCase().includes(needle);
}

export function countViews(tasks: Iterable<Task>, categories: string[]): Record<string, number> {
  const c: Record<string, number> = { all: 0, active: 0, queued: 0, completed: 0, failed: 0 };
  for (const id of categories) c[`cat:${id}`] = 0;
  for (const t of tasks) {
    c.all++;
    if (t.status === "starting" || t.status === "active") c.active++;
    else if (t.status === "queued" || t.status === "paused") c.queued++;
    else if (t.status === "completed") c.completed++;
    else if (t.status === "failed") c.failed++;
    const k = `cat:${t.category}`;
    if (k in c) c[k]++;
  }
  return c;
}

/** 0–100, or null when the size is unknown (indeterminate bar, never a fake %). */
export function percent(t: Pick<Task, "done" | "total" | "status">): number | null {
  if (t.status === "completed") return 100;
  if (!t.total || t.total <= 0) return null;
  return Math.min(100, (t.done / t.total) * 100);
}

export function etaSeconds(t: Pick<Task, "done" | "total" | "speed">): number | null {
  if (!t.total || t.speed <= 0 || t.done >= t.total) return null;
  return Math.ceil((t.total - t.done) / t.speed);
}

export function displayName(t: Task): string {
  return t.title || t.filename || t.url;
}

/** Sort by queue position (stable for equal positions). */
export const byPosition = (a: Task, b: Task) => a.position - b.position || a.id - b.id;

export const canPause = (t: Task) => t.status === "queued" || t.status === "starting" || t.status === "active";
export const canResume = (t: Task) => t.status === "paused" || t.status === "failed";
export const canRetry = (t: Task) => t.status === "failed" || t.status === "cancelled" || t.status === "completed";
export const canCancel = (t: Task) => !["completed", "cancelled"].includes(t.status);
