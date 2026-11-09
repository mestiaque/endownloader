import { create } from "zustand";

export interface Toast {
  id: number;
  kind: "info" | "success" | "error" | "progress";
  title: string;
  detail?: string;
  progress?: { done: number; total: number };
  onCancel?: () => void;
  action?: { label: string; run: () => void };
}

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

export interface PromptRequest {
  title: string;
  label?: string;
  initial: string;
  confirmLabel?: string;
  /** Select only the stem of a file name initially. */
  selectStem?: boolean;
  resolve: (value: string | null) => void;
}

export type DialogKind = null | "settings" | "shortcuts";

interface UiStore {
  toasts: Toast[];
  confirmReq: ConfirmRequest | null;
  promptReq: PromptRequest | null;
  dialog: DialogKind;
  settingsSection: string;
  toast(t: Omit<Toast, "id">, ttl?: number): number;
  updateToast(id: number, t: Partial<Toast>): void;
  dismiss(id: number): void;
  openDialog(d: DialogKind, section?: string): void;
}

let toastId = 0;

export const useUi = create<UiStore>((set, get) => ({
  toasts: [],
  confirmReq: null,
  promptReq: null,
  dialog: null,
  settingsSection: "general",
  toast(t, ttl = 4000) {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { ...t, id }].slice(-5) });
    if (t.kind !== "progress" && ttl > 0) setTimeout(() => get().dismiss(id), ttl);
    return id;
  },
  updateToast(id, t) {
    set({ toasts: get().toasts.map((x) => (x.id === id ? { ...x, ...t } : x)) });
    if (t.kind && t.kind !== "progress") setTimeout(() => get().dismiss(id), 4000);
  },
  dismiss(id) {
    set({ toasts: get().toasts.filter((x) => x.id !== id) });
  },
  openDialog(d, section) {
    set({ dialog: d, ...(section ? { settingsSection: section } : {}) });
  },
}));

export const toast = (t: Omit<Toast, "id">, ttl?: number) => useUi.getState().toast(t, ttl);

export function confirmDialog(opts: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
  return new Promise((resolve) => useUi.setState({ confirmReq: { ...opts, resolve } }));
}

export function promptDialog(opts: Omit<PromptRequest, "resolve">): Promise<string | null> {
  return new Promise((resolve) => useUi.setState({ promptReq: { ...opts, resolve } }));
}
