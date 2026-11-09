import { AlertTriangle, Check, Clock, Download, ExternalLink, Film, ListVideo, MonitorPlay, Music, RefreshCw, User, X } from "lucide-react";
import { useMemo, useState } from "react";
import { api, errorMessage } from "../../lib/api";
import { cn, formatBytes } from "../../lib/format";
import type { MediaOption } from "../../lib/types";
import { downloadEntries, downloadOption, useAnalyze, type Card } from "../../store/analyze";
import { useUi } from "../../store/ui";
import { toast } from "../../store/ui";
import { Button, Select, Spinner } from "../ui/primitives";

export function formatDuration(sec: number | null | undefined): string {
  if (!sec || sec <= 0) return "—";
  const s = Math.round(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${r}` : `${m}:${r}`;
}

const sizeText = (o: MediaOption) => (o.size ? `${o.sizeEstimated ? "~" : ""}${formatBytes(o.size)}` : "—");

function Thumb({ src, duration, live }: { src: string | null | undefined; duration?: number | null; live?: boolean }) {
  const [broken, setBroken] = useState(false);
  return (
    <div className="relative aspect-video w-full overflow-hidden rounded-lg border border-line bg-raised">
      {src && !broken ? (
        <img src={src} alt="" className="h-full w-full object-cover" referrerPolicy="no-referrer" onError={() => setBroken(true)} />
      ) : (
        <div className="flex h-full items-center justify-center text-faint">
          <Film className="h-8 w-8" />
        </div>
      )}
      {live ? (
        <span className="absolute bottom-1.5 right-1.5 rounded bg-danger px-1.5 py-0.5 text-[10px] font-semibold text-white">LIVE</span>
      ) : duration ? (
        <span className="absolute bottom-1.5 right-1.5 rounded bg-black/75 px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-white">{formatDuration(duration)}</span>
      ) : null}
    </div>
  );
}

function Meta({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex gap-3 text-[12px]">
      <span className="w-[70px] shrink-0 text-faint">{label}</span>
      <span className="min-w-0 truncate">{value}</span>
    </div>
  );
}

function CardError({ card }: { card: Card }) {
  const retry = () => useAnalyze.getState().analyze([card.url], card.info?.isPlaylist);
  const update = async () => {
    try {
      toast({ kind: "info", title: "Updating yt-dlp…" });
      const t = await api.installTool("yt-dlp");
      toast({ kind: "success", title: `yt-dlp ${t.version ?? ""} installed` });
      retry();
    } catch (e) {
      toast({ kind: "error", title: "Could not update yt-dlp", detail: errorMessage(e) });
    }
  };
  // yt-dlp found no video or audio: the link may simply be a file behind a page-like URL.
  const asFile = async () => {
    try {
      const r = await api.addLinks(card.url, { asFile: true, allowDuplicates: true });
      if (r.added.length) {
        toast({ kind: "success", title: "Downloading as a file", detail: card.url });
        useAnalyze.getState().close(card.key);
      }
    } catch (e) {
      toast({ kind: "error", title: "Could not add the link", detail: errorMessage(e) });
    }
  };
  return (
    <div className="flex items-start gap-3 rounded-lg border border-danger/30 bg-danger/5 p-3">
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-danger" />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium">{card.errorKind === "drm" ? "Protected content — not supported" : "Could not read this link"}</div>
        <div className="mt-0.5 break-words text-[12px] text-muted">{card.error}</div>
        {card.errorKind === "loginRequired" && <div className="mt-1 text-[12px] text-faint">Sign in to the site in your browser, then pick that browser in Settings → Media → Cookies.</div>}
      </div>
      <div className="flex shrink-0 gap-1.5">
        {card.errorKind === "loginRequired" && (
          <Button size="sm" variant="default" onClick={() => useUi.getState().openDialog("settings")}>
            Settings
          </Button>
        )}
        {card.errorKind === "toolOutdated" && (
          <Button size="sm" variant="primary" onClick={update}>
            Update yt-dlp
          </Button>
        )}
        {card.errorKind !== "drm" && card.errorKind !== "loginRequired" && (
          <Button size="sm" variant="default" onClick={asFile} title="Save what this link returns, without looking for video or audio">
            <Download className="h-3.5 w-3.5" />
            Download as a file
          </Button>
        )}
        {card.errorKind !== "drm" && (
          <Button size="sm" variant="default" onClick={retry}>
            <RefreshCw className="h-3.5 w-3.5" />
            Retry
          </Button>
        )}
      </div>
    </div>
  );
}

/** "Full HD", "4K"… for a video height. */
function qualityBadge(h: number | null): string | null {
  if (!h) return null;
  if (h >= 4320) return "8K";
  if (h >= 2160) return "4K";
  if (h >= 1440) return "2K";
  if (h >= 1080) return "FHD";
  if (h >= 720) return "HD";
  return null;
}

/** "Best (1080p)" → "1080p"; other labels unchanged. */
const shortLabel = (o: MediaOption) => o.label.replace(/^Best \((.+)\)$/, "$1");

/**
 * The options shown as tiles. "Best (1080p)" and a plain "1080p" with the same resolution
 * and size are the same download for the user: keep the Best one (its format string falls
 * back more gracefully) and mark it with a badge.
 */
function dedupe(options: MediaOption[]): { list: MediaOption[]; best: MediaOption | null } {
  const best = options.find((o) => o.label.startsWith("Best")) ?? null;
  const list = options.filter((o) => o === best || !best || !(o.resolution === best.resolution && o.ext === best.ext && (o.size === best.size || !o.size || !best.size)));
  return { list, best };
}

/** A value shared by every option ("MP4", "25 fps"…), shown once instead of in every tile. */
function common<T>(options: MediaOption[], pick: (o: MediaOption) => T | null): T | null {
  const vals = options.map(pick).filter((v) => v != null) as T[];
  return vals.length === options.length && vals.every((v) => v === vals[0]) ? vals[0] : null;
}

/** Link without tracking/playlist noise, for display only. */
function displayUrl(url: string): string {
  try {
    const u = new URL(url);
    for (const k of ["list", "start_radio", "index", "pp", "si", "feature", "utm_source", "utm_medium", "utm_campaign"]) u.searchParams.delete(k);
    return `${u.host.replace(/^www\./, "")}${u.pathname}${u.search}`;
  } catch {
    return url;
  }
}

function QualityTile({ o, best, selected, done, onSelect, onDownload, showExt, showFps }: {
  o: MediaOption; best: boolean; selected: boolean; done: boolean; onSelect: () => void; onDownload: () => void; showExt: boolean; showFps: boolean;
}) {
  const badge = o.kind === "video" ? qualityBadge(o.height) : null;
  const sub = [showExt ? o.ext : null, showFps && o.fps ? `${o.fps} fps` : null, sizeText(o)].filter(Boolean).join(" · ");
  return (
    <button
      onClick={onSelect}
      onDoubleClick={onDownload}
      title={`${o.label}${o.resolution ? ` · ${o.resolution}` : ""}${o.codecs ? ` · ${o.codecs}` : ""} — double-click to download`}
      className={cn(
        "group relative flex min-w-0 flex-col items-start rounded-lg border px-3 py-2 text-left transition-colors",
        selected ? "border-accent bg-accent/10 ring-1 ring-accent" : "border-line bg-raised/40 hover:border-faint hover:bg-raised",
      )}
    >
      <span className="flex w-full items-center gap-1.5">
        <span className="text-[14px] font-semibold tabular-nums">{shortLabel(o)}</span>
        {badge && <span className="rounded bg-fg/10 px-1 text-[10px] font-bold tracking-wide text-muted">{badge}</span>}
        {o.fps && o.fps > 30 && <span className="rounded bg-fg/10 px-1 text-[10px] font-bold text-muted">{o.fps}</span>}
        <span className="flex-1" />
        {done && <Check className="h-3.5 w-3.5 shrink-0 text-success" />}
      </span>
      <span className="mt-0.5 w-full truncate text-[11px] tabular-nums text-muted">{sub || "\u00a0"}</span>
      {best && <span className="absolute -top-2 right-2 rounded-full bg-accent px-1.5 py-px text-[9px] font-bold tracking-wider text-accent-fg shadow-sm">BEST</span>}
    </button>
  );
}

function SingleCard({ card }: { card: Card }) {
  const info = card.info!;
  const [descOpen, setDescOpen] = useState(false);
  const video = useMemo(() => dedupe(info.video), [info.video]);
  const audio = useMemo(() => dedupe(info.audio), [info.audio]);
  const [tab, setTab] = useState<"video" | "audio">(info.video.length ? "video" : "audio");
  const list = tab === "video" ? video.list : audio.list;
  const [pickedLabel, setPickedLabel] = useState<string | null>(null);
  const selected = list.find((o) => o.label === pickedLabel) ?? (tab === "video" ? video.best : audio.best) ?? list[0];
  const isDone = (o: MediaOption) => card.queued.has(o.label + o.kind);
  const mp3 = info.audio.find((o) => o.extractAudio === "mp3");
  const sharedExt = common(list, (o) => o.ext);
  const sharedFps = common(list, (o) => o.fps);
  const sharedCodecs = common(list, (o) => o.codecs);
  const shared = [sharedExt, sharedCodecs, sharedFps ? `${sharedFps} fps` : null].filter(Boolean).join(" · ");

  return (
    <div className="grid grid-cols-[260px_minmax(0,1fr)] gap-5">
      <div className="min-w-0">
        <Thumb src={info.thumbnail} duration={info.duration} live={info.isLive} />
        {info.playlistId && (
          <Button size="sm" variant="ghost" className="mt-2 w-full justify-center" onClick={() => useAnalyze.getState().analyze([info.url], true)}>
            <ListVideo className="h-3.5 w-3.5" />
            Show the whole playlist
          </Button>
        )}
      </div>

      <div className="flex min-w-0 flex-col gap-3">
        <div>
          <div className="line-clamp-2 text-[15px] font-semibold leading-snug" title={info.title}>
            {info.title}
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-muted">
            <span className="flex items-center gap-1 rounded-full bg-raised px-2 py-0.5">
              <MonitorPlay className="h-3 w-3" />
              {info.platform}
            </span>
            {info.uploader && (
              <span className="flex max-w-[220px] items-center gap-1 truncate rounded-full bg-raised px-2 py-0.5">
                <User className="h-3 w-3 shrink-0" />
                <span className="truncate">{info.uploader}</span>
              </span>
            )}
            <span className="flex items-center gap-1 rounded-full bg-raised px-2 py-0.5 tabular-nums">
              <Clock className="h-3 w-3" />
              {info.isLive ? "Live" : formatDuration(info.duration)}
            </span>
            {info.description && (
              <button className="px-1 text-faint hover:text-fg" onClick={() => setDescOpen(!descOpen)}>
                {descOpen ? "Hide description" : "Description"}
              </button>
            )}
          </div>
          {descOpen && <div className="mt-2 max-h-28 overflow-y-auto whitespace-pre-wrap break-words rounded-lg bg-raised/50 p-2 text-[11px] text-muted">{info.description}</div>}
        </div>

        {/* Main action: the selected quality, plus one-click MP3. */}
        <div className="flex flex-wrap items-center gap-2">
          {selected && (
            <Button variant="primary" className="h-9 px-4" onClick={() => void downloadOption(card, selected)}>
              {isDone(selected) ? <Check className="h-4 w-4" /> : <Download className="h-4 w-4" />}
              {isDone(selected) ? "Added · add again" : `Download ${shortLabel(selected)}`}
              <span className="font-normal opacity-80">· {selected.ext}{selected.size ? ` · ${sizeText(selected)}` : ""}</span>
            </Button>
          )}
          {mp3 && tab === "video" && (
            <Button className="h-9" onClick={() => void downloadOption(card, mp3)} title="Audio only, converted to MP3">
              {isDone(mp3) ? <Check className="h-4 w-4 text-success" /> : <Music className="h-4 w-4 text-success" />}
              MP3{mp3.size ? <span className="text-muted">· {sizeText(mp3)}</span> : null}
            </Button>
          )}
        </div>

        {/* Quality picker */}
        <div>
          <div className="mb-2 flex items-center gap-2">
            <div className="inline-flex rounded-md border border-line bg-panel p-0.5">
              {(["video", "audio"] as const).map((t) => {
                const n = (t === "video" ? video.list : audio.list).length;
                if (!n) return null;
                return (
                  <button
                    key={t}
                    onClick={() => { setTab(t); setPickedLabel(null); }}
                    className={cn("flex h-7 items-center gap-1.5 rounded px-2.5 text-[12px]", tab === t ? "bg-raised text-fg" : "text-muted hover:text-fg")}
                  >
                    {t === "video" ? <Film className="h-3.5 w-3.5" /> : <Music className="h-3.5 w-3.5" />}
                    {t === "video" ? "Video" : "Audio"}
                    <span className="text-faint">{n}</span>
                  </button>
                );
              })}
            </div>
            {shared && <span className="truncate text-[11px] text-faint" title={shared}>All {tab}: {shared}</span>}
          </div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(96px,1fr))] gap-2 pt-1">
            {list.map((o) => (
              <QualityTile
                key={o.label + o.kind}
                o={o}
                best={o === (tab === "video" ? video.best : audio.best)}
                selected={o === selected}
                done={isDone(o)}
                showExt={!sharedExt}
                showFps={!sharedFps}
                onSelect={() => setPickedLabel(o.label)}
                onDownload={() => void downloadOption(card, o)}
              />
            ))}
          </div>
          <div className="mt-1.5 text-[11px] text-faint">Click a quality to choose it · double-click to download it straight away</div>
        </div>
      </div>
    </div>
  );
}

function PlaylistCard({ card }: { card: Card }) {
  const info = card.info!;
  const picked = card.picked ?? new Set<string>();
  const options = useMemo(() => [...info.video, ...info.audio.map((o) => ({ ...o, label: o.label === "Best" ? "Audio (M4A)" : o.label }))], [info]);
  const [choice, setChoice] = useState(0);
  const all = picked.size === info.entries.length;
  const toggle = (id: string) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    useAnalyze.getState().patch(card.key, { picked: next });
  };
  const selected = info.entries.filter((e) => picked.has(e.id));
  return (
    <div className="grid grid-cols-[230px_minmax(0,1fr)] gap-5">
      <div className="min-w-0 space-y-2.5">
        <Thumb src={info.thumbnail} />
        <div className="line-clamp-3 text-[14px] font-semibold leading-snug">{info.title}</div>
        <div className="space-y-1">
          <Meta label="Platform" value={info.platform} />
          <Meta label="Items" value={info.entries.length} />
          {info.uploader && <Meta label="Uploader" value={info.uploader} />}
        </div>
      </div>
      <div className="flex min-w-0 flex-col gap-2">
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-2 text-[12px]">
            <input type="checkbox" className="accent-[rgb(var(--accent))]" checked={all} onChange={() => useAnalyze.getState().patch(card.key, { picked: all ? new Set() : new Set(info.entries.map((e) => e.id)) })} />
            Select all
          </label>
          <span className="text-[12px] text-faint">
            {selected.length} of {info.entries.length} selected
          </span>
          <div className="flex-1" />
          <Select value={choice} onChange={(v) => setChoice(v)} options={options.map((o, i) => ({ value: i, label: o.kind === "audio" ? o.label : `Video · ${o.label}` }))} />
          <Button variant="primary" size="sm" disabled={!selected.length} onClick={() => void downloadEntries(card, selected, options[choice])}>
            <Download className="h-3.5 w-3.5" />
            Download {selected.length || ""}
          </Button>
        </div>
        <div className="max-h-[260px] overflow-y-auto rounded-lg border border-line">
          {info.entries.map((e, i) => (
            <label key={e.id + i} className="flex cursor-pointer items-center gap-3 border-b border-line/50 px-3 py-1.5 text-[12px] last:border-0 hover:bg-raised/40">
              <input type="checkbox" className="accent-[rgb(var(--accent))]" checked={picked.has(e.id)} onChange={() => toggle(e.id)} />
              <span className="w-7 shrink-0 text-right tabular-nums text-faint">{i + 1}</span>
              <span className="min-w-0 flex-1 truncate" title={e.title}>
                {e.title}
              </span>
              <span className="shrink-0 tabular-nums text-faint">{formatDuration(e.duration)}</span>
            </label>
          ))}
        </div>
      </div>
    </div>
  );
}

function AnalyzeCard({ card }: { card: Card }) {
  return (
    <div className="rounded-xl border border-line bg-panel p-4 shadow-pop">
      <div className="mb-3 flex items-center gap-2">
        <button className="flex min-w-0 items-center gap-1.5 text-[11px] text-faint underline-offset-2 hover:text-fg hover:underline" title="Open in the browser" onClick={() => void api.openUrl(card.url).catch(() => {})}>
          <span className="truncate">{displayUrl(card.url)}</span>
          <ExternalLink className="h-3 w-3 shrink-0" />
        </button>
        <div className="flex-1" />
        <button className="rounded p-1 text-faint hover:bg-raised hover:text-fg" aria-label="Close" title="Close" onClick={() => useAnalyze.getState().close(card.key)}>
          <X className="h-4 w-4" />
        </button>
      </div>
      {card.state === "loading" && (
        <div className="grid grid-cols-[230px_minmax(0,1fr)] gap-5">
          <div className="aspect-video animate-pulse rounded-lg bg-raised" />
          <div className="flex items-center gap-2 text-[13px] text-muted">
            <Spinner className="h-4 w-4" />
            Reading the page and the available qualities…
          </div>
        </div>
      )}
      {card.state === "error" && <CardError card={card} />}
      {card.state === "ready" && card.info && (card.info.isPlaylist ? <PlaylistCard card={card} /> : <SingleCard card={card} />)}
    </div>
  );
}

/** Analysis cards above the download list (one per pasted media link). */
export function AnalyzePanel() {
  const cards = useAnalyze((s) => s.cards);
  if (!cards.length) return null;
  return (
    <div className="max-h-[62%] shrink-0 space-y-3 overflow-y-auto border-b border-line bg-bg/60 p-3">
      {cards.length > 1 && (
        <div className="flex items-center justify-between px-1 text-[12px] text-muted">
          <span>{cards.length} links</span>
          <button className="hover:text-fg" onClick={() => useAnalyze.getState().closeAll()}>
            Close all
          </button>
        </div>
      )}
      {cards.map((c) => (
        <AnalyzeCard key={c.key} card={c} />
      ))}
    </div>
  );
}
