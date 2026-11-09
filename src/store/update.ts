import { create } from "zustand";
import { api, errorMessage, type UpdateInfo } from "../lib/api";
import { toast } from "./ui";

const LAST_CHECK_KEY = "ensave.lastUpdateCheck";
// A little under a day, so a computer started at about the same time every day still checks daily.
const DAY = 20 * 3600 * 1000;

interface UpdateStore {
  available: UpdateInfo | null;
  dialogOpen: boolean;
  checking: boolean;
  /** Ask GitHub for a newer release. Manual checks report "up to date" and errors too. */
  check(manual: boolean): Promise<string>;
  open(): void;
  close(): void;
}

function lastCheck(): number {
  try {
    return Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0);
  } catch {
    return 0;
  }
}

export const useUpdate = create<UpdateStore>((set, get) => ({
  available: null,
  dialogOpen: false,
  checking: false,
  async check(manual) {
    if (get().checking) return "Checking…";
    set({ checking: true });
    try {
      const info = await api.checkUpdate();
      try {
        localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
      } catch {
        /* storage unavailable: just check again next start */
      }
      set({ available: info });
      if (!info) return "You have the latest version";
      if (manual) set({ dialogOpen: true });
      else toast({ kind: "info", title: `ENSave ${info.version} is available`, action: { label: "Update", run: () => get().open() } }, 30000);
      return `ENSave ${info.version} is available`;
    } catch (e) {
      const msg = "Could not check for updates. Are you online?";
      if (manual) toast({ kind: "error", title: msg, detail: errorMessage(e) });
      return msg;
    } finally {
      set({ checking: false });
    }
  },
  open: () => set({ dialogOpen: true }),
  close: () => set({ dialogOpen: false }),
}));

/** Quiet check a few seconds after start-up, at most about once a day. */
export function scheduleUpdateCheck(enabled: boolean) {
  if (!enabled || Date.now() - lastCheck() < DAY) return () => {};
  const t = window.setTimeout(() => void useUpdate.getState().check(false), 8000);
  return () => window.clearTimeout(t);
}
