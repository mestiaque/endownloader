import { listen } from "@tauri-apps/api/event";
import { useMemo } from "react";
import { create } from "zustand";
import { byPosition, inView, matchesSearch, type ViewId } from "../engines/taskView";
import { api, errorMessage, type TaskAction } from "../lib/api";
import type { AddResult, ProgressItem, Task } from "../lib/types";
import { useAnalyze } from "./analyze";
import { useSettings } from "./settings";
import { confirmDialog, toast } from "./ui";

interface TaskStore {
  /** All tasks, sorted by queue position. */
  tasks: Task[];
  byId: Map<number, Task>;
  view: ViewId;
  search: string;
  selection: Set<number>;
  /** Anchor for shift-click range selection and keyboard focus. */
  focused: number | null;
  loaded: boolean;
  init(): Promise<() => void>;
  setView(v: ViewId): void;
  setSearch(q: string): void;
  select(id: number, mode: "single" | "toggle" | "range", visible: Task[]): void;
  setSelection(ids: number[]): void;
  clearSelection(): void;
  upsert(tasks: Task[]): void;
  applyProgress(items: ProgressItem[]): void;
  removeLocal(ids: number[]): void;
}

export const useTasks = create<TaskStore>((set, get) => ({
  tasks: [],
  byId: new Map(),
  view: "all",
  search: "",
  selection: new Set(),
  focused: null,
  loaded: false,

  async init() {
    const list = await api.listTasks();
    set({ tasks: [...list].sort(byPosition), byId: new Map(list.map((t) => [t.id, t])), loaded: true });
    const un1 = listen<Task[]>("tasks-upsert", (e) => get().upsert(e.payload));
    const un2 = listen<number[]>("tasks-removed", (e) => get().removeLocal(e.payload));
    const un3 = listen<ProgressItem[]>("tasks-progress", (e) => get().applyProgress(e.payload));
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      un3.then((f) => f());
    };
  },

  setView(view) {
    set({ view, selection: new Set(), focused: null });
  },
  setSearch(search) {
    set({ search });
  },

  select(id, mode, visible) {
    const { selection, focused } = get();
    if (mode === "single") return set({ selection: new Set([id]), focused: id });
    if (mode === "toggle") {
      const next = new Set(selection);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return set({ selection: next, focused: id });
    }
    const a = visible.findIndex((t) => t.id === focused);
    const b = visible.findIndex((t) => t.id === id);
    if (a < 0 || b < 0) return set({ selection: new Set([id]), focused: id });
    const [lo, hi] = a < b ? [a, b] : [b, a];
    set({ selection: new Set(visible.slice(lo, hi + 1).map((t) => t.id)) });
  },
  setSelection(ids) {
    set({ selection: new Set(ids) });
  },
  clearSelection() {
    set({ selection: new Set() });
  },

  /** Progress updates keep the array order; new tasks or moved ones re-sort it. */
  upsert(incoming) {
    const byId = new Map(get().byId);
    let resort = false;
    for (const t of incoming) {
      const old = byId.get(t.id);
      if (!old || old.position !== t.position) resort = true;
      byId.set(t.id, t);
    }
    const tasks = resort ? [...byId.values()].sort(byPosition) : get().tasks.map((t) => byId.get(t.id) ?? t);
    set({ tasks, byId });
  },

  /** Live progress (≤ 4 batches/s): patch the rows in place, order unchanged. */
  applyProgress(items) {
    const byId = new Map(get().byId);
    let changed = false;
    for (const p of items) {
      const old = byId.get(p.id);
      if (!old) continue;
      byId.set(p.id, { ...old, done: p.done, total: p.total ?? old.total, speed: p.speed, connections: p.connections });
      changed = true;
    }
    if (changed) set({ byId, tasks: get().tasks.map((t) => byId.get(t.id) ?? t) });
  },

  removeLocal(ids) {
    const gone = new Set(ids);
    const byId = new Map(get().byId);
    ids.forEach((id) => byId.delete(id));
    const selection = new Set([...get().selection].filter((id) => !gone.has(id)));
    set({ tasks: get().tasks.filter((t) => !gone.has(t.id)), byId, selection, focused: gone.has(get().focused ?? -1) ? null : get().focused });
  },
}));

/** Tasks shown in the current view (sidebar entry + search). */
export function useVisibleTasks(): Task[] {
  const tasks = useTasks((s) => s.tasks);
  const view = useTasks((s) => s.view);
  const search = useTasks((s) => s.search);
  return useMemo(() => tasks.filter((t) => inView(t, view) && matchesSearch(t, search)), [tasks, view, search]);
}

export function visibleSnapshot(): Task[] {
  const { tasks, view, search } = useTasks.getState();
  return tasks.filter((t) => inView(t, view) && matchesSearch(t, search));
}

/** Selected tasks, or the focused one when nothing is selected. */
export function targetTasks(): Task[] {
  const { selection, focused, byId } = useTasks.getState();
  if (selection.size) return [...selection].map((id) => byId.get(id)).filter((t): t is Task => !!t);
  const f = focused != null ? byId.get(focused) : undefined;
  return f ? [f] : [];
}

// ---------- actions ----------

export async function addLinks(text: string, opts: { folder?: string | null; quality?: string | null } = {}): Promise<number> {
  if (!text.trim()) return 0;
  try {
    return reportAdded(await api.addLinks(text, opts), opts);
  } catch (e) {
    toast({ kind: "error", title: "Could not add links", detail: errorMessage(e) });
    return 0;
  }
}

/** Add the links found in a text file (Import, or a dropped .txt file). */
export async function importLinksFile(path: string): Promise<number> {
  try {
    return reportAdded(await api.importLinks(path), {});
  } catch (e) {
    toast({ kind: "error", title: "Could not import links", detail: errorMessage(e) });
    return 0;
  }
}

function reportAdded(r: AddResult, opts: { folder?: string | null; quality?: string | null }): number {
  {
    if (r.added.length) {
      toast({ kind: "success", title: r.added.length === 1 ? "Added 1 download" : `Added ${r.added.length} downloads` });
      useTasks.getState().setSelection(r.added.map((t) => t.id));
    }
    if (r.duplicates.length) {
      toast(
        {
          kind: "info",
          title: r.duplicates.length === 1 ? "This link is already in the list" : `${r.duplicates.length} links are already in the list`,
          detail: r.duplicates.slice(0, 3).join("\n") + (r.duplicates.length > 3 ? "\n…" : ""),
          action: {
            label: "Add anyway",
            run: () => void api.addLinks(r.duplicates.join("\n"), { ...opts, allowDuplicates: true }).catch((e) => toast({ kind: "error", title: "Could not add", detail: errorMessage(e) })),
          },
        },
        8000,
      );
    }
    if (r.media.length) useAnalyze.getState().analyze(r.media);
    if (r.invalid.length) toast({ kind: "error", title: `${r.invalid.length} invalid link(s) skipped`, detail: r.invalid.slice(0, 3).join("\n") });
    if (!r.added.length && !r.duplicates.length && !r.invalid.length && !r.media.length) toast({ kind: "info", title: "No links found", detail: "Paste one or more links (http, https, ftp or magnet)." });
    return r.added.length + r.media.length;
  }
}

export async function runAction(tasks: Task[], action: TaskAction) {
  if (!tasks.length) return;
  try {
    await api.taskAction(
      tasks.map((t) => t.id),
      action,
    );
  } catch (e) {
    toast({ kind: "error", title: "Action failed", detail: errorMessage(e) });
  }
}

export async function removeTasks(tasks: Task[], deleteFiles = false) {
  if (!tasks.length) return;
  if (useSettings.getState().settings.confirmRemove || deleteFiles) {
    const n = tasks.length;
    const ok = await confirmDialog({
      title: deleteFiles ? `Delete ${n === 1 ? "this download" : `${n} downloads`} and the files?` : `Remove ${n === 1 ? "this download" : `${n} downloads`} from the list?`,
      message: deleteFiles ? "The downloaded files are deleted from disk. This cannot be undone." : "Downloaded files stay on disk.",
      confirmLabel: deleteFiles ? "Delete files" : "Remove",
      danger: deleteFiles,
    });
    if (!ok) return;
  }
  try {
    await api.removeTasks(
      tasks.map((t) => t.id),
      deleteFiles,
    );
  } catch (e) {
    toast({ kind: "error", title: "Could not remove", detail: errorMessage(e) });
  }
}
