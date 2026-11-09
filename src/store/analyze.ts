/** Analysis cards: one per media link, like the web demo (thumbnail, title, format tables). */
import { create } from "zustand";
import { api, errorMessage, type MediaItem } from "../lib/api";
import type { MediaInfo, MediaOption, PlaylistEntry } from "../lib/types";
import { toast } from "./ui";

export interface Card {
  key: string;
  url: string;
  state: "loading" | "ready" | "error";
  info?: MediaInfo;
  error?: string;
  errorKind?: string;
  /** Playlist cards: which entries are ticked. */
  picked?: Set<string>;
  /** Labels of options already queued from this card (button shows a tick). */
  queued: Set<string>;
}

interface AnalyzeStore {
  cards: Card[];
  analyze(urls: string[], playlist?: boolean): void;
  close(key: string): void;
  closeAll(): void;
  patch(key: string, p: Partial<Card>): void;
}

let seq = 0;

export const useAnalyze = create<AnalyzeStore>((set, get) => ({
  cards: [],

  analyze(urls, playlist = false) {
    for (const url of urls) {
      // The same link again: refresh the existing card instead of adding a second one.
      const existing = get().cards.find((c) => c.url === url && !!c.info?.isPlaylist === playlist);
      const key = existing?.key ?? `c${++seq}`;
      const card: Card = { key, url, state: "loading", queued: new Set() };
      set({ cards: existing ? get().cards.map((c) => (c.key === key ? card : c)) : [card, ...get().cards] });
      api
        .analyzeUrl(url, playlist)
        .then((info) => get().patch(key, { state: "ready", info, picked: info.isPlaylist ? new Set(info.entries.map((e) => e.id)) : undefined }))
        .catch((e) => get().patch(key, { state: "error", error: errorMessage(e), errorKind: (e as { kind?: string })?.kind }));
    }
  },
  close(key) {
    set({ cards: get().cards.filter((c) => c.key !== key) });
  },
  closeAll() {
    set({ cards: [] });
  },
  patch(key, p) {
    set({ cards: get().cards.map((c) => (c.key === key ? { ...c, ...p } : c)) });
  },
}));

/** Queue one option of a single-video card. */
export async function downloadOption(card: Card, option: MediaOption) {
  const info = card.info;
  if (!info) return;
  try {
    await api.addMedia([{ url: info.url, title: info.title, thumbnail: info.thumbnail, mediaId: info.id }], option);
    useAnalyze.getState().patch(card.key, { queued: new Set([...card.queued, option.label + option.kind]) });
    toast({ kind: "success", title: `Downloading ${option.kind === "audio" ? `audio (${option.label})` : option.label}`, detail: info.title });
  } catch (e) {
    toast({ kind: "error", title: "Could not start the download", detail: errorMessage(e) });
  }
}

/** Queue the ticked entries of a playlist card with one quality. */
export async function downloadEntries(card: Card, entries: PlaylistEntry[], option: MediaOption) {
  if (!entries.length) return;
  const items: MediaItem[] = entries.map((e) => ({ url: e.url, title: e.title, thumbnail: e.thumbnail, mediaId: e.id }));
  try {
    const added = await api.addMedia(items, option);
    useAnalyze.getState().patch(card.key, { queued: new Set([...card.queued, option.label + option.kind]) });
    toast({ kind: "success", title: `Added ${added.length} download${added.length === 1 ? "" : "s"}`, detail: `${card.info?.title ?? "Playlist"} · ${option.label}` });
  } catch (e) {
    toast({ kind: "error", title: "Could not add the playlist", detail: errorMessage(e) });
  }
}
