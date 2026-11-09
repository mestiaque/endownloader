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

/** One row of the Video / Audio table of an analysed link. */
export interface MediaOption {
  kind: "video" | "audio";
  label: string;
  ext: string;
  resolution: string | null;
  height: number | null;
  fps: number | null;
  codecs: string | null;
  size: number | null;
  sizeEstimated: boolean;
  format: string;
  merge: string | null;
  extractAudio: string | null;
}

export interface PlaylistEntry {
  id: string;
  title: string;
  url: string;
  duration: number | null;
  thumbnail: string | null;
}

export interface MediaInfo {
  url: string;
  id: string;
  title: string;
  thumbnail: string | null;
  duration: number | null;
  uploader: string | null;
  platform: string;
  isLive: boolean;
  description: string | null;
  video: MediaOption[];
  audio: MediaOption[];
  entries: PlaylistEntry[];
  isPlaylist: boolean;
  playlistId: string | null;
}

export interface ToolInfo {
  id: "yt-dlp" | "ffmpeg" | "deno" | "node" | "gallery-dl";
  path: string | null;
  version: string | null;
  managed: boolean;
  installable: boolean;
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
  mediaQuality: "best" | "2160" | "1440" | "1080" | "720" | "480" | "360" | "audio" | "mp3";
  cookiesBrowser: "" | "firefox" | "chrome" | "chromium" | "edge" | "brave" | "opera" | "vivaldi";
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
  /** Media-site links: shown as analysis cards so the user can pick a format. */
  media: string[];
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
