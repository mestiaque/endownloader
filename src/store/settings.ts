import { create } from "zustand";
import { api, errorMessage } from "../lib/api";
import type { Settings } from "../lib/types";
import { toast } from "./ui";

/** Used until the backend answers (the backend owns the real defaults). */
export const DEFAULT_SETTINGS: Settings = {
  startWithOs: false,
  startMinimized: false,
  closeToTray: true,
  confirmRemove: true,
  notifyOnFinish: true,
  downloadDir: "",
  maxParallel: 3,
  connections: 8,
  conflict: "rename",
  detectDuplicates: true,
  useCategories: true,
  keepPartial: false,
  retries: 3,
  categories: [],
  theme: "dark",
  accent: "#3b82f6",
  animations: true,
  sidebarCollapsed: false,
  language: "en",
};

interface SettingsStore {
  settings: Settings;
  systemDownloads: string;
  loaded: boolean;
  load(): Promise<void>;
  set<K extends keyof Settings>(key: K, value: Settings[K]): Promise<void>;
  replace(s: Settings): void;
}

export const useSettings = create<SettingsStore>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  systemDownloads: "",
  loaded: false,
  async load() {
    try {
      const r = await api.getSettings();
      set({ settings: r.settings, systemDownloads: r.systemDownloads, loaded: true });
    } catch (e) {
      console.error("settings load failed", e);
      set({ loaded: true });
    }
  },
  async set(key, value) {
    const prev = get().settings;
    if (prev[key] === value) return;
    set({ settings: { ...prev, [key]: value } }); // optimistic
    try {
      // The backend validates (clamps numbers etc.) and returns the stored settings.
      set({ settings: await api.setSetting(key, value) });
    } catch (e) {
      set({ settings: prev });
      toast({ kind: "error", title: "Could not save setting", detail: errorMessage(e) });
    }
  },
  replace(s) {
    set({ settings: s });
  },
}));

export const setting = <K extends keyof Settings>(k: K) => useSettings.getState().settings[k];
