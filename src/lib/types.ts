/** Mirrors of the Rust types (endl-core). */

export type TaskStatus = "queued" | "starting" | "active" | "paused" | "completed" | "failed" | "cancelled";
export type EngineKind = "auto" | "aria2" | "media" | "gallery";

export interface Task {
  id: number;
  url: string;
  engine: EngineKind;
  status: TaskStatus;
  title: string | null;
  filename: string | null;
  folder: string;
  category: string;
  /** null = size unknown → indeterminate progress. */
  total: number | null;
  done: number;
  speed: number;
  connections: number;
  error: string | null;
  errorKind: string | null;
  priority: number;
  position: number;
  createdAt: number;
  finishedAt: number | null;
  options: unknown;
}

/** Live progress batch item (`tasks-progress` event). */
export interface ProgressItem {
  id: number;
  done: number;
  total: number | null;
  speed: number;
  connections: number;
  etaSecs: number | null;
}

export interface Category {
  id: string;
  name: string;
  extensions: string[];
  folder: string | null;
}

export interface Settings {
  startWithOs: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
  confirmRemove: boolean;
  notifyOnFinish: boolean;
  downloadDir: string;
  maxParallel: number;
  connections: number;
  conflict: "rename" | "overwrite" | "skip" | "ask";
  detectDuplicates: boolean;
  useCategories: boolean;
  keepPartial: boolean;
  retries: number;
  categories: Category[];
  theme: "dark" | "light" | "glass" | "system";
  accent: string;
  animations: boolean;
  sidebarCollapsed: boolean;
  language: "en" | "bn";
}

export interface AddResult {
  added: Task[];
  duplicates: string[];
  invalid: string[];
}

export interface CliRequest {
  urls: string[];
  folder: string | null;
  quality: string | null;
  minimized: boolean;
}

export interface AppPaths {
  data: string;
  logs: string;
  downloads: string;
  portable: boolean;
}
