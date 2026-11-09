/** Typed wrappers around the Rust commands. One function per command. */
import { invoke } from "@tauri-apps/api/core";
import type { AddResult, AppPaths, CliRequest, MediaInfo, MediaOption, Settings, Task, ToolInfo } from "./types";

export interface MediaItem {
  url: string;
  title?: string | null;
  thumbnail?: string | null;
  mediaId?: string | null;
}

/** A newer release on GitHub (see src-tauri/src/commands/update.rs). */
export interface UpdateInfo {
  version: string;
  notes: string;
  page: string;
  assetName: string;
  url: string;
  size: number;
  sha256: string;
  installable: boolean;
}

export type TaskAction = "pause" | "resume" | "cancel" | "retry";

export const api = {
  getSettings: () => invoke<{ settings: Settings; systemDownloads: string }>("get_settings"),
  setSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => invoke<Settings>("set_setting", { key, value }),
  resetSettings: () => invoke<Settings>("reset_settings"),
  exportSettings: (path: string) => invoke<void>("export_settings", { path }),
  importSettings: (path: string) => invoke<Settings>("import_settings", { path }),

  listTasks: () => invoke<Task[]>("list_tasks"),
  addLinks: (text: string, opts: { folder?: string | null; quality?: string | null; allowDuplicates?: boolean } = {}) =>
    invoke<AddResult>("add_links", { text, folder: opts.folder ?? null, quality: opts.quality ?? null, allowDuplicates: opts.allowDuplicates ?? false }),
  analyzeUrl: (url: string, playlist = false) => invoke<MediaInfo>("analyze_url", { url, playlist }),
  addMedia: (items: MediaItem[], option: MediaOption, folder?: string | null) => invoke<Task[]>("add_media", { items, option, folder: folder ?? null }),
  toolsStatus: () => invoke<ToolInfo[]>("tools_status"),
  installTool: (id: string) => invoke<ToolInfo>("install_tool", { id }),
  importLinks: (path: string) => invoke<AddResult>("import_links", { path }),
  clearCompleted: () => invoke<number>("clear_completed"),
  taskAction: (ids: number[], action: TaskAction) => invoke<Task[]>("task_action", { ids, action }),
  pauseAll: () => invoke<number>("pause_all"),
  resumeAll: () => invoke<number>("resume_all"),
  removeTasks: (ids: number[], deleteFiles: boolean) => invoke<number>("remove_tasks", { ids, deleteFiles }),
  reorderTasks: (ids: number[]) => invoke<void>("reorder_tasks", { ids }),
  setPriority: (ids: number[], priority: number) => invoke<void>("set_priority", { ids, priority }),

  getLaunchRequest: () => invoke<CliRequest | null>("get_launch_request"),
  appPaths: () => invoke<AppPaths>("app_paths"),
  readLogs: (maxLines = 500) => invoke<string>("read_logs", { maxLines }),
  diagnostics: () => invoke<string>("diagnostics"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  openLogFolder: () => invoke<void>("open_log_folder"),
  quitApp: () => invoke<void>("quit_app"),
  hideToTray: () => invoke<void>("hide_to_tray"),

  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: (info: UpdateInfo) => invoke<"restart" | "exiting">("install_update", { info }),
  restartApp: () => invoke<void>("restart_app"),
  openReleasePage: () => invoke<void>("open_release_page"),
};

/** Commands reject with `{ kind, message }` (DlError) or a plain string. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
