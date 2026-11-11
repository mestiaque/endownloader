import { AlertTriangle, Check, Download, Eye, ExternalLink, Film, Images, ListVideo, Music, RefreshCw, X } from "lucide-react";
import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { api, errorMessage } from "../../lib/api";
import { cn, formatBytes } from "../../lib/format";
import type { MediaOption, PlaylistEntry } from "../../lib/types";
import { downloadEntries, downloadOption, useAnalyze, type Card } from "../../store/analyze";
import { useSettings } from "../../store/settings";
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

function DownloadButton({ done, onClick }: { done: boolean; onClick: () => void }) {
  return (
    <Button size="sm" variant={done ? "ghost" : "default"} onClick={onClick} className="min-w-[104px] justify-center" title={done ? "Added to the queue — click to add again" : "Add to the queue"}>
      {done ? <Check className="h-3.5 w-3.5 text-success" /> : <Download className="h-3.5 w-3.5" />}
      {done ? "Added" : "Download"}
    </Button>
  );
}

function Th({ children, right, w }: { children?: React.ReactNode; right?: boolean; w?: string }) {
  return <th className={cn("px-2 py-1.5 text-[11px] font-semibold text-muted", right ? "text-right" : "text-left", w)}>{children}</th>;
}

/**
 * A thumbnail that can stay blurred (Settings → Media → Blur previews) until it is pressed
 * and held. `enlarge`: while held, also show it big in the middle of the window (list rows).
 */
function HoldImage({ src, enlarge, className }: { src: string; enlarge?: boolean; className?: string }) {
  const blur = useSettings((s) => s.settings.blurPreviews);
  const [held, setHeld] = useState(false);
  const [broken, setBroken] = useState(false);
  if (broken) return <Film className="h-1/2 w-1/2 text-faint" />;
  const release = () => setHeld(false);
  return (
    <div
      className={cn("relative h-full w-full select-none overflow-hidden", (blur || enlarge) && "cursor-zoom-in", className)}
      title={held ? undefined : blur ? "Press and hold to see" : enlarge ? "Press and hold to enlarge" : undefined}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.stopPropagation();
        e.currentTarget.setPointerCapture(e.pointerId);
        setHeld(true);
      }}
      onPointerUp={release}
      onPointerCancel={release}
      onLostPointerCapture={release}
      // Inside a list row's <label>: holding the picture must not tick the checkbox.
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <img
        src={src}
        alt=""
        draggable={false}
        loading="lazy"
        decoding="async"
        referrerPolicy="no-referrer"
        onError={() => setBroken(true)}
        className={cn("h-full w-full object-cover transition-[filter,transform] duration-150", blur && !held && "scale-110 blur-md")}
      />
      {blur && !held && (
        <span className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <span className="rounded-full bg-black/55 p-1 text-white">
            <Eye className="h-3.5 w-3.5" />
          </span>
        </span>
      )}
      {enlarge &&
        held &&
        // On <body>: list rows use content-visibility, which would clip a fixed overlay to the row.
        createPortal(
          <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-black/40">
            <img src={src} alt="" referrerPolicy="no-referrer" className="max-h-[75vh] max-w-[70vw] rounded-lg object-contain shadow-2xl" />
          </div>,
          document.body,
        )}
    </div>
  );
}

function Thumb({ src, duration, live }: { src: string | null | undefined; duration?: number | null; live?: boolean }) {
  return (
    <div className="relative aspect-video w-full overflow-hidden rounded-lg border border-line bg-raised">
      {src ? (
        <HoldImage key={src} src={src} />
      ) : (
        <div className="flex h-full items-center justify-center text-faint">
          <Film className="h-8 w-8" />
        </div>
      )}
      {live ? (
        <span className="pointer-events-none absolute bottom-1.5 right-1.5 rounded bg-danger px-1.5 py-0.5 text-[10px] font-semibold text-white">LIVE</span>
      ) : duration ? (
        <span className="pointer-events-none absolute bottom-1.5 right-1.5 rounded bg-black/75 px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-white">{formatDuration(duration)}</span>
      ) : null}
    </div>
  );
}

/** Small thumbnail of one playlist/profile entry. */
function RowThumb({ src }: { src: string | null }) {
  return (
    <div className="flex h-9 w-16 shrink-0 items-center justify-center overflow-hidden rounded border border-line bg-raised">
      {src ? <HoldImage key={src} src={src} enlarge /> : <Film className="h-4 w-4 text-faint" />}
    </div>
  );
}

function TypeBadge({ e }: { e: PlaylistEntry }) {
  if (e.kind === "photo")
    return (
      <span className="flex shrink-0 items-center gap-1 rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] font-medium text-amber-500" title="Photo post: its pictures are saved as JPG files">
        <Images className="h-3 w-3" />
        Photos
      </span>
    );
  return (
    <span className="flex shrink-0 items-center gap-1 rounded-full bg-accent/15 px-2 py-0.5 text-[11px] font-medium text-accent">
      <Film className="h-3 w-3" />
      {e.section && e.section !== "Videos" ? e.section : "Video"}
    </span>
  );
}

function Meta({ label, value, wrap }: { label: string; value: React.ReactNode; wrap?: boolean }) {
  return (
    <div className="flex gap-3 text-[12px]">
      <span className="w-[70px] shrink-0 text-faint">{label}</span>
      <span className={cn("min-w-0", wrap ? "break-words" : "truncate")}>{value}</span>
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
  // 403: the site (TikTok above all) refuses anything that is not a signed-in browser.
  const needsCookies = card.errorKind === "loginRequired" || card.errorKind === "forbidden";
  return (
    <div className="flex items-start gap-3 rounded-lg border border-danger/30 bg-danger/5 p-3">
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-danger" />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium">{card.errorKind === "drm" ? "Protected content — not supported" : "Could not read this link"}</div>
        <div className="mt-0.5 break-words text-[12px] text-muted">{card.error}</div>
        {needsCookies && <div className="mt-1 text-[12px] text-faint">Sign in to the site in your browser, then pick that browser in Settings → Media → Cookies.</div>}
      </div>
      <div className="flex shrink-0 gap-1.5">
        {needsCookies && (
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

function SingleCard({ card }: { card: Card }) {
  const info = card.info!;
  const [descOpen, setDescOpen] = useState(false);
  const isDone = (o: MediaOption) => card.queued.has(o.label + o.kind);
  return (
    <div className="grid grid-cols-[230px_minmax(0,1fr)] gap-5">
      <div className="min-w-0 space-y-2.5">
        <Thumb src={info.thumbnail} duration={info.duration} live={info.isLive} />
        <div className="line-clamp-3 text-[14px] font-semibold leading-snug" title={info.title}>
          {info.title}
        </div>
        <div className="space-y-1">
          <Meta label="Platform" value={info.platform} />
          <Meta label="Duration" value={info.isLive ? "Live" : formatDuration(info.duration)} />
          {info.uploader && <Meta label="Uploader" value={info.uploader} />}
        </div>
        {info.playlistId && (
          <Button size="sm" variant="default" className="w-full justify-center" onClick={() => useAnalyze.getState().analyze([info.url], true)}>
            <ListVideo className="h-3.5 w-3.5" />
            Show the whole playlist
          </Button>
        )}
        {info.description && (
          <div>
            <button className="text-[12px] text-muted hover:text-fg" onClick={() => setDescOpen(!descOpen)}>
              {descOpen ? "▾" : "▸"} Description
            </button>
            {descOpen && <div className="mt-1 max-h-40 overflow-y-auto whitespace-pre-wrap break-words text-[11px] text-muted">{info.description}</div>}
          </div>
        )}
      </div>
      <div className="min-w-0 space-y-4">
        {info.video.length > 0 && (
          <section>
            <h3 className="mb-1 flex items-center gap-1.5 text-[13px] font-semibold">
              <Film className="h-4 w-4 text-accent" /> Video
            </h3>
            <table className="w-full table-fixed border-collapse">
              <thead className="border-b border-line">
                <tr>
                  <Th w="w-[118px]">Quality</Th>
                  <Th w="w-[62px]">Format</Th>
                  <Th w="w-[92px]">Resolution</Th>
                  <Th w="w-[44px]">FPS</Th>
                  <Th>Codecs</Th>
                  <Th w="w-[78px]">Size</Th>
                  <th className="w-[116px]" />
                </tr>
              </thead>
              <tbody>
                {info.video.map((o) => (
                  <tr key={o.label} className="border-b border-line/50 text-[12px] hover:bg-raised/40">
                    <td className="truncate px-2 py-1.5 font-semibold" title={o.label}>{o.label}</td>
                    <td className="px-2">{o.ext}</td>
                    <td className="px-2 tabular-nums text-muted">{o.resolution ?? "—"}</td>
                    <td className="px-2 tabular-nums text-muted">{o.fps ?? "—"}</td>
                    <td className="truncate px-2 text-muted" title={o.codecs ?? undefined}>
                      {o.codecs ?? "—"}
                    </td>
                    <td className="px-2 tabular-nums">{sizeText(o)}</td>
                    <td className="py-1 text-right">
                      <DownloadButton done={isDone(o)} onClick={() => void downloadOption(card, o)} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}
        {info.audio.length > 0 && (
          <section>
            <h3 className="mb-1 flex items-center gap-1.5 text-[13px] font-semibold">
              <Music className="h-4 w-4 text-success" /> Audio
            </h3>
            <table className="w-full table-fixed border-collapse">
              <thead className="border-b border-line">
                <tr>
                  <Th>Quality</Th>
                  <Th>Format</Th>
                  <Th>Codec</Th>
                  <Th>Size</Th>
                  <th className="w-[116px]" />
                </tr>
              </thead>
              <tbody>
                {info.audio.map((o) => (
                  <tr key={o.label} className="border-b border-line/50 text-[12px] hover:bg-raised/40">
                    <td className="truncate px-2 py-1.5 font-semibold" title={o.label}>{o.label}</td>
                    <td className="px-2">{o.ext}</td>
                    <td className="px-2 text-muted">{o.codecs ?? "—"}</td>
                    <td className="px-2 tabular-nums">{sizeText(o)}</td>
                    <td className="py-1 text-right">
                      <DownloadButton done={isDone(o)} onClick={() => void downloadOption(card, o)} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}
      </div>
    </div>
  );
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** What a profile/channel holds: "66 videos · 14 photo posts", "200 Videos · 1495 Shorts". */
function countText(entries: PlaylistEntry[]): string {
  const photos = entries.filter((e) => e.kind === "photo").length;
  const sections = new Map<string, number>();
  for (const e of entries) if (e.kind === "video" && e.section) sections.set(e.section, (sections.get(e.section) ?? 0) + 1);
  const parts = sections.size > 1 ? [...sections].map(([name, n]) => `${n} ${name}`) : [plural(entries.length - photos, "video")];
  if (photos) parts.push(plural(photos, "photo post"));
  return parts.join(" · ");
}

/** Quick picks next to "Select all": by type (TikTok photo posts) or by channel tab. */
function quickPicks(entries: PlaylistEntry[]): { label: string; icon: typeof Film; ids: string[] }[] {
  const ids = (f: (e: PlaylistEntry) => boolean) => entries.filter(f).map((e) => e.id);
  if (entries.some((e) => e.kind === "photo"))
    return [
      { label: "Only videos", icon: Film, ids: ids((e) => e.kind === "video") },
      { label: "Only photos", icon: Images, ids: ids((e) => e.kind === "photo") },
    ];
  const sections = [...new Set(entries.map((e) => e.section).filter((x): x is string => !!x))];
  return sections.length > 1 ? sections.map((name) => ({ label: `Only ${name.toLowerCase()}`, icon: Film, ids: ids((e) => e.section === name) })) : [];
}

function PlaylistCard({ card }: { card: Card }) {
  const info = card.info!;
  const picked = card.picked ?? new Set<string>();
  const options = useMemo(() => [...info.video, ...info.audio.map((o) => ({ ...o, label: o.label === "Best" ? "Audio (M4A)" : o.label }))], [info]);
  const picks = useMemo(() => quickPicks(info.entries), [info]);
  const [choice, setChoice] = useState(0);
  const [zip, setZip] = useState(() => {
    try {
      return localStorage.getItem("ensave.zipBulk") === "1";
    } catch {
      return false;
    }
  });
  const toggleZip = (on: boolean) => {
    setZip(on);
    try {
      localStorage.setItem("ensave.zipBulk", on ? "1" : "0");
    } catch {
      /* private mode: just this card */
    }
  };
  const all = picked.size === info.entries.length;
  const setPicked = (ids: Iterable<string>) => useAnalyze.getState().patch(card.key, { picked: new Set(ids) });
  const toggle = (id: string) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setPicked(next);
  };
  const selected = info.entries.filter((e) => picked.has(e.id));
  return (
    <div className="grid grid-cols-[230px_minmax(0,1fr)] gap-5">
      <div className="min-w-0 space-y-2.5">
        <Thumb src={info.thumbnail} />
        <div className="line-clamp-3 text-[14px] font-semibold leading-snug">{info.title}</div>
        <div className="space-y-1">
          <Meta label="Platform" value={info.platform} />
          <Meta label="Items" value={countText(info.entries)} wrap />
          {info.uploader && <Meta label="Uploader" value={info.uploader} />}
        </div>
      </div>
      <div className="flex min-w-0 flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-2 text-[12px]">
            <input type="checkbox" className="accent-[rgb(var(--accent))]" checked={all} onChange={() => setPicked(all ? [] : info.entries.map((e) => e.id))} />
            Select all
          </label>
          {picks.length > 0 && (
            <div className="flex overflow-hidden rounded-md border border-line">
              {picks.map((p) => {
                const Icon = p.icon;
                const active = p.ids.length > 0 && p.ids.length === picked.size && p.ids.every((id) => picked.has(id));
                return (
                  <button
                    key={p.label}
                    className={cn("flex items-center gap-1 border-r border-line px-2 py-1 text-[11px] last:border-0", active ? "bg-accent/15 text-accent" : "text-muted hover:bg-raised hover:text-fg")}
                    title={`Tick only these (${p.ids.length})`}
                    onClick={() => setPicked(p.ids)}
                  >
                    <Icon className="h-3 w-3" />
                    {p.label}
                  </button>
                );
              })}
            </div>
          )}
          <span className="text-[12px] text-faint">
            {selected.length} of {info.entries.length} selected
          </span>
          <div className="flex-1" />
          <Select value={choice} onChange={(v) => setChoice(v)} options={options.map((o, i) => ({ value: i, label: o.kind === "audio" ? o.label : `Video · ${o.label}` }))} />
          <label className="flex items-center gap-1.5 text-[12px]" title={`Download into one folder, then pack it into "${info.title}.zip" when every item is done`}>
            <input type="checkbox" className="accent-[rgb(var(--accent))]" checked={zip} onChange={(e) => toggleZip(e.target.checked)} />
            One ZIP
          </label>
          <Button variant="primary" size="sm" disabled={!selected.length} onClick={() => void downloadEntries(card, selected, options[choice], zip)}>
            <Download className="h-3.5 w-3.5" />
            Download {selected.length || ""}
          </Button>
        </div>
        <div className="max-h-[360px] overflow-y-auto rounded-lg border border-line">
          {info.entries.map((e, i) => (
            <label key={e.id + i} className="flex cursor-pointer items-center gap-3 border-b [contain-intrinsic-size:auto_49px] [content-visibility:auto] border-line/50 px-3 py-1.5 text-[12px] last:border-0 hover:bg-raised/40">
              <input type="checkbox" className="accent-[rgb(var(--accent))]" checked={picked.has(e.id)} onChange={() => toggle(e.id)} />
              <span className="w-7 shrink-0 text-right tabular-nums text-faint">{i + 1}</span>
              <RowThumb src={e.thumbnail} />
              <span className="min-w-0 flex-1 truncate" title={e.title}>
                {e.title || `Item ${i + 1}`}
              </span>
              <TypeBadge e={e} />
              <span className="w-12 shrink-0 text-right tabular-nums text-faint">{formatDuration(e.duration)}</span>
            </label>
          ))}
        </div>
      </div>
    </div>
  );
}

function AnalyzeCard({ card }: { card: Card }) {
  return (
    <div className="rounded-xl border border-line bg-panel p-4 shadow-sm">
      <div className="mb-3 flex items-center gap-2">
        <button className="flex min-w-0 items-center gap-1.5 text-[12px] text-muted underline-offset-2 hover:text-fg hover:underline" title="Open in the browser" onClick={() => void api.openUrl(card.url).catch(() => {})}>
          <span className="truncate">{card.url}</span>
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
          <div className="flex flex-col justify-center gap-1">
            <div className="flex items-center gap-2 text-[13px] text-muted">
              <Spinner className="h-4 w-4" />
              Reading the page and the available qualities…
            </div>
            <div className="pl-6 text-[11px] text-faint">Profiles and channels with many posts can take a minute or two.</div>
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
  const listCollapsed = useUi((s) => s.listCollapsed);
  if (!cards.length) return null;
  return (
    <div className={cn("space-y-3 overflow-y-auto bg-bg/60 p-3", listCollapsed ? "min-h-0 flex-1" : "max-h-[62%] shrink-0 border-b border-line")}>
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
