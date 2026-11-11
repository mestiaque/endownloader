import { getVersion } from "@tauri-apps/api/app";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Check, Copy, FolderOpen, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, errorMessage } from "../../lib/api";
import { cn } from "../../lib/format";
import type { Category, Settings, ToolInfo } from "../../lib/types";
import { useSettings } from "../../store/settings";
import { confirmDialog, toast, useUi } from "../../store/ui";
import { Dialog } from "../ui/overlays";
import { Button, Input, Select, Switch } from "../ui/primitives";
import { CategoryIcon } from "../tasks/categoryIcon";
import { useUpdate } from "../../store/update";
import { AboutSection } from "./AboutSection";

const SECTIONS = [
  ["general", "General"],
  ["downloads", "Downloads"],
  ["categories", "Categories"],
  ["media", "Media"],
  ["tools", "Tools"],
  ["appearance", "Appearance"],
  ["logs", "Logs"],
  ["about", "About"],
] as const;

const THEME_CARDS: { id: Settings["theme"]; label: string; bg: string; side: string; bar: string; tile: string }[] = [
  { id: "dark", label: "Dark", bg: "#131315", side: "#19191c", bar: "#2c2c32", tile: "#222226" },
  { id: "light", label: "Light", bg: "#f4f4f6", side: "#fcfcfd", bar: "#dcdce2", tile: "#ffffff" },
  {
    id: "glass",
    label: "Glass",
    bg: "radial-gradient(circle at 15% 20%, rgba(139,108,255,.9), transparent 55%), radial-gradient(circle at 90% 10%, rgba(226,64,122,.8), transparent 50%), radial-gradient(circle at 70% 95%, rgba(56,189,248,.75), transparent 55%), #1a1530",
    side: "rgba(255,255,255,.14)",
    bar: "rgba(255,255,255,.3)",
    tile: "rgba(255,255,255,.18)",
  },
  { id: "system", label: "System", bg: "linear-gradient(135deg, #f4f4f6 50%, #131315 50%)", side: "rgba(127,127,127,.25)", bar: "rgba(127,127,127,.4)", tile: "rgba(127,127,127,.3)" },
];

/** Theme picker with small previews (same design as ENViewer and ENPlayer). */
function ThemeCards({ value, onChange }: { value: Settings["theme"]; onChange: (v: Settings["theme"]) => void }) {
  return (
    <div className="border-b border-line/60 py-3">
      <div className="text-[13px]">Theme</div>
      <div className="mt-2.5 grid grid-cols-4 gap-3">
        {THEME_CARDS.map((t) => (
          <button
            key={t.id}
            onClick={() => onChange(t.id)}
            className={cn("rounded-[10px] border p-2 pb-1.5 text-[12px] transition-colors", value === t.id ? "border-accent bg-accent/[0.07] text-fg ring-1 ring-accent" : "border-line text-muted hover:border-faint")}
          >
            <div className="flex h-[70px] overflow-hidden rounded-md border border-black/10" style={{ background: t.bg }}>
              <div className="w-[26%]" style={{ background: t.side }} />
              <div className="flex flex-1 flex-col gap-1.5 p-1.5">
                <div className="h-2 rounded-sm" style={{ background: t.bar }} />
                <div className="flex flex-1 flex-col gap-1">
                  {[0, 1, 2].map((i) => (
                    <div key={i} className="flex-1 rounded-[3px]" style={{ background: t.tile }} />
                  ))}
                </div>
              </div>
            </div>
            <div className="mt-1.5">{t.label}</div>
          </button>
        ))}
      </div>
      <div className="mt-2 text-[11px] text-faint">Glass uses frosted, blurred panels over a colour gradient.</div>
    </div>
  );
}

function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-line/60 py-3 last:border-0">
      <div className="min-w-0">
        <div className="text-[13px]">{label}</div>
        {hint && <div className="mt-0.5 break-words text-[11px] text-faint">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

type BoolKey = { [K in keyof Settings]: Settings[K] extends boolean ? K : never }[keyof Settings];

function Toggle({ k, label, hint }: { k: BoolKey; label: string; hint?: string }) {
  const { settings, set } = useSettings();
  return (
    <Field label={label} hint={hint}>
      <Switch checked={settings[k]} onChange={(v) => void set(k, v)} label={label} />
    </Field>
  );
}

const ACCENTS = ["#3b82f6", "#6366f1", "#8b5cf6", "#ec4899", "#ef4444", "#f97316", "#eab308", "#22c55e", "#14b8a6", "#06b6d4"];

async function chooseFolder(title: string): Promise<string | null> {
  const d = await openDialog({ directory: true, title });
  return typeof d === "string" ? d : null;
}

function UpdateRow() {
  const check = useUpdate((u) => u.check);
  const checking = useUpdate((u) => u.checking);
  const [status, setStatus] = useState("");
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion, () => {});
  }, []);
  return (
    <Field label="Updates" hint={status || `You have version ${version}`}>
      <Button size="sm" disabled={checking} onClick={async () => setStatus(await check(true))}>
        {checking ? "Checking…" : "Check now"}
      </Button>
    </Field>
  );
}

function GeneralSection() {
  const replace = useSettings((s) => s.replace);
  return (
    <>
      <Toggle k="startWithOs" label="Start with the system" hint="ENSave starts in the tray when you log in, so queued downloads continue" />
      <Toggle k="startMinimized" label="Start minimized to the tray" />
      <Toggle k="closeToTray" label="Keep running in the tray when the window is closed" hint="Downloads continue in the background. Quit from the tray menu" />
      <Toggle k="confirmRemove" label="Confirm before removing downloads from the list" />
      <Toggle k="checkUpdates" label="Check for updates automatically" hint="Looks for a new ENSave release on GitHub once a day" />
      <UpdateRow />
      <Field label="Settings file" hint="Move your settings to another computer">
        <div className="flex gap-2">
          <Button
            size="sm"
            onClick={async () => {
              const p = await openDialog({ filters: [{ name: "Settings", extensions: ["json"] }] });
              if (typeof p !== "string") return;
              try {
                replace(await api.importSettings(p));
                toast({ kind: "success", title: "Settings imported" });
              } catch (e) {
                toast({ kind: "error", title: "Could not import settings", detail: errorMessage(e) });
              }
            }}
          >
            Import…
          </Button>
          <Button
            size="sm"
            onClick={async () => {
              const p = await saveDialog({ defaultPath: "ensave-settings.json", filters: [{ name: "Settings", extensions: ["json"] }] });
              if (!p) return;
              try {
                await api.exportSettings(p);
                toast({ kind: "success", title: "Settings exported" });
              } catch (e) {
                toast({ kind: "error", title: "Could not export settings", detail: errorMessage(e) });
              }
            }}
          >
            Export…
          </Button>
        </div>
      </Field>
      <Field label="Reset all settings" hint="Downloads and history are kept">
        <Button
          size="sm"
          variant="danger"
          onClick={async () => {
            if (!(await confirmDialog({ title: "Reset all settings?", message: "Every setting goes back to its default. Your downloads and history are kept.", confirmLabel: "Reset", danger: true }))) return;
            replace(await api.resetSettings());
            toast({ kind: "success", title: "Settings reset" });
          }}
        >
          Reset
        </Button>
      </Field>
    </>
  );
}

function DownloadsSection() {
  const { settings: s, set, systemDownloads } = useSettings();
  return (
    <>
      <Field label="Download folder" hint={s.downloadDir || `${systemDownloads} (system Downloads folder)`}>
        <div className="flex gap-2">
          {s.downloadDir && (
            <Button size="sm" variant="ghost" onClick={() => void set("downloadDir", "")}>
              Use default
            </Button>
          )}
          <Button
            size="sm"
            onClick={async () => {
              const d = await chooseFolder("Download folder");
              if (d) void set("downloadDir", d);
            }}
          >
            <FolderOpen className="h-3.5 w-3.5" />
            Choose…
          </Button>
        </div>
      </Field>
      <Field label="Downloads at the same time" hint="Others wait in the queue">
        <Select value={s.maxParallel} onChange={(v) => void set("maxParallel", v)} options={Array.from({ length: 10 }, (_, i) => ({ value: i + 1, label: String(i + 1) }))} />
      </Field>
      <Field label="Connections per download" hint="More connections are faster on servers that allow it (8 is a good default)">
        <Select value={s.connections} onChange={(v) => void set("connections", v)} options={Array.from({ length: 16 }, (_, i) => ({ value: i + 1, label: String(i + 1) }))} />
      </Field>
      <Field label="Retry failed downloads" hint="Network errors and busy servers are retried with growing waits (5 s, 10 s, 20 s…)">
        <Select value={s.retries} onChange={(v) => void set("retries", v)} options={[0, 1, 2, 3, 5, 10].map((n) => ({ value: n, label: n === 0 ? "Never" : `${n} time${n > 1 ? "s" : ""}` }))} />
      </Field>
      <Field label="When the file already exists">
        <Select
          value={s.conflict}
          onChange={(v) => void set("conflict", v)}
          options={[
            { value: "rename", label: "Rename the new file" },
            { value: "overwrite", label: "Overwrite" },
            { value: "skip", label: "Skip the download" },
          ]}
        />
      </Field>
      <Toggle k="detectDuplicates" label="Warn about links that are already in the list" />
      <Toggle k="useCategories" label="Sort files into category folders" hint="Video, Audio, Images… inside the download folder (Settings → Categories)" />
      <Toggle k="keepPartial" label="Keep partial files when a download is cancelled" />
    </>
  );
}

function CategoryRow({ c, onChange, onDelete }: { c: Category; onChange: (c: Category) => void; onDelete?: () => void }) {
  const [exts, setExts] = useState(c.extensions.join(" "));
  useEffect(() => setExts(c.extensions.join(" ")), [c.extensions]);
  return (
    <div className="border-b border-line/60 py-3">
      <div className="flex items-center gap-2">
        <span className="text-accent">
          <CategoryIcon id={c.id} />
        </span>
        <Input className="h-7 w-40" value={c.name} onChange={(e) => onChange({ ...c, name: e.target.value })} aria-label="Category name" />
        <span className="min-w-0 flex-1 truncate text-[11px] text-faint" title={c.folder ?? undefined}>
          {c.id === "other" ? "Saved in the download folder" : c.folder ? c.folder : `Folder: ${c.name}/ in the download folder`}
        </span>
        {c.id !== "other" && (
          <>
            {c.folder && (
              <Button size="sm" variant="ghost" onClick={() => onChange({ ...c, folder: null })}>
                Default
              </Button>
            )}
            <Button
              size="sm"
              onClick={async () => {
                const d = await chooseFolder(`Folder for ${c.name}`);
                if (d) onChange({ ...c, folder: d });
              }}
            >
              Folder…
            </Button>
          </>
        )}
        {onDelete && (
          <button className="rounded p-1 text-faint hover:bg-raised hover:text-danger" aria-label={`Delete ${c.name}`} onClick={onDelete}>
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
      {c.id !== "other" && (
        <Input
          className="mt-2 h-7 font-mono text-[12px]"
          value={exts}
          placeholder="Extensions, e.g. mp4 mkv webm"
          aria-label={`${c.name} extensions`}
          onChange={(e) => setExts(e.target.value)}
          onBlur={() => onChange({ ...c, extensions: exts.split(/[\s,]+/).filter(Boolean) })}
        />
      )}
    </div>
  );
}

const BUILTIN = new Set(["video", "audio", "images", "archives", "programs", "iso", "documents", "other"]);

function CategoriesSection() {
  const { settings: s, set } = useSettings();
  const timer = useRef<number>();
  const [draft, setDraft] = useState(s.categories);
  useEffect(() => setDraft(s.categories), [s.categories]);
  // Names are saved shortly after typing stops; other edits immediately.
  const update = (next: Category[], debounce = false) => {
    setDraft(next);
    window.clearTimeout(timer.current);
    if (debounce) timer.current = window.setTimeout(() => void set("categories", next), 500);
    else void set("categories", next);
  };
  return (
    <>
      <div className="pb-1 pt-3 text-[11px] text-faint">Files are sorted by extension. A category without its own folder uses a sub-folder of the download folder.</div>
      {draft.map((c, i) => (
        <CategoryRow
          key={c.id}
          c={c}
          onChange={(nc) => update(
            draft.map((x, j) => (j === i ? nc : x)),
            nc.name !== c.name,
          )}
          onDelete={BUILTIN.has(c.id) ? undefined : () => update(draft.filter((_, j) => j !== i))}
        />
      ))}
      <div className="py-3">
        <Button
          size="sm"
          onClick={() => {
            const id = `custom-${Date.now().toString(36)}`;
            const other = draft.findIndex((c) => c.id === "other");
            const next = [...draft];
            next.splice(other < 0 ? next.length : other, 0, { id, name: "New category", extensions: [], folder: null });
            update(next);
          }}
        >
          <Plus className="h-3.5 w-3.5" />
          Add category
        </Button>
      </div>
    </>
  );
}

function MediaSection() {
  const { settings: s, set } = useSettings();
  return (
    <>
      <div className="py-3 text-[12px] text-muted">
        Links from YouTube, TikTok, Instagram, Facebook, X and many more sites open an analysis card where you pick the quality. These settings apply where no card is shown: the
        command line and playlists.
      </div>
      <Field label="Default quality" hint="Used by the command line (ensave &quot;link&quot;) and as the first choice for playlists">
        <Select
          value={s.mediaQuality}
          onChange={(v) => void set("mediaQuality", v)}
          options={[
            { value: "best", label: "Best available" },
            { value: "2160", label: "Up to 4K (2160p)" },
            { value: "1440", label: "Up to 1440p" },
            { value: "1080", label: "Up to 1080p" },
            { value: "720", label: "Up to 720p" },
            { value: "480", label: "Up to 480p" },
            { value: "360", label: "Up to 360p" },
            { value: "audio", label: "Audio only (M4A)" },
            { value: "mp3", label: "Audio only (MP3)" },
          ]}
        />
      </Field>
      <Field
        label="Cookies from browser"
        hint="For private, age-restricted or members-only media your account can see. Sign in to the site in that browser; ENSave reads the cookies only while downloading and never stores them."
      >
        <Select
          value={s.cookiesBrowser}
          onChange={(v) => void set("cookiesBrowser", v)}
          options={[
            { value: "", label: "Don't use cookies" },
            { value: "firefox", label: "Firefox" },
            { value: "chrome", label: "Google Chrome" },
            { value: "chromium", label: "Chromium" },
            { value: "edge", label: "Microsoft Edge" },
            { value: "brave", label: "Brave" },
            { value: "opera", label: "Opera" },
            { value: "vivaldi", label: "Vivaldi" },
          ]}
        />
      </Field>
      <Toggle k="blurPreviews" label="Blur previews" hint="Thumbnails on analysis cards stay blurred until you press and hold one" />
      <div className="py-3 text-[11px] text-faint">DRM-protected content (Netflix, Disney+, Spotify and similar) is not supported and is reported as such.</div>
    </>
  );
}

const IS_WINDOWS = navigator.userAgent.includes("Windows");

const TOOL_TEXT: Record<ToolInfo["id"], { name: string; what: string; missing: string }> = {
  "yt-dlp": { name: "yt-dlp", what: "Reads media sites and downloads video and audio; kept up to date automatically", missing: "Downloaded automatically when you are online" },
  ffmpeg: {
    name: "FFmpeg",
    what: "Joins video and audio, converts to MP3",
    missing: IS_WINDOWS ? "Downloaded automatically when you are online" : "Install it with: sudo apt install ffmpeg",
  },
  deno: { name: "Deno", what: "JavaScript runtime yt-dlp uses for YouTube (preferred)", missing: "Downloaded automatically when Node.js is not installed either" },
  node: { name: "Node.js", what: "JavaScript runtime yt-dlp can use for YouTube", missing: "Optional — without Deno or Node.js some YouTube formats may be missing" },
  "gallery-dl": { name: "gallery-dl", what: "Pictures of photo posts (TikTok slideshows)", missing: "Installed automatically the first time a photo post is downloaded" },
};

function ToolsSection() {
  const [tools, setTools] = useState<ToolInfo[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const load = () => api.toolsStatus().then(setTools, (e) => toast({ kind: "error", title: "Could not check the tools", detail: errorMessage(e) }));
  useEffect(() => {
    void load();
  }, []);
  const install = async (id: string) => {
    setBusy(id);
    try {
      const t = await api.installTool(id);
      toast({ kind: "success", title: `${TOOL_TEXT[t.id].name} ${t.version ?? ""} installed` });
      await load();
    } catch (e) {
      toast({ kind: "error", title: "Install failed", detail: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  };
  if (!tools) return <div className="py-6 text-[13px] text-muted">Checking tools…</div>;
  return (
    <>
      {tools.map((t) => (
        <Field
          key={t.id}
          label={`${TOOL_TEXT[t.id].name}${t.version ? ` ${t.version}` : ""}`}
          hint={
            t.path ? (
              <>
                {TOOL_TEXT[t.id].what}
                <br />
                <span className="font-mono">{t.path}</span>
                {t.managed && " (installed by ENSave)"}
              </>
            ) : (
              <>
                <span className="text-danger">Not found.</span> {TOOL_TEXT[t.id].missing}
              </>
            )
          }
        >
          {t.installable ? (
            <Button size="sm" variant={t.path ? "default" : "primary"} disabled={busy !== null} onClick={() => void install(t.id)} title="Download the latest official release (checksum verified)">
              <RefreshCw className={cn("h-3.5 w-3.5", busy === t.id && "animate-spin")} />
              {busy === t.id ? "Downloading…" : t.managed ? "Update" : t.path ? "Install latest" : "Install"}
            </Button>
          ) : (
            <span className={cn("text-[12px]", t.path ? "text-success" : "text-faint")}>{t.path ? "Found" : "—"}</span>
          )}
        </Field>
      ))}
      <div className="flex justify-end py-3">
        <Button size="sm" variant="ghost" onClick={() => void load()}>
          <RefreshCw className="h-3.5 w-3.5" />
          Check again
        </Button>
      </div>
    </>
  );
}

function AppearanceSection() {
  const { settings: s, set } = useSettings();
  return (
    <>
      <ThemeCards value={s.theme} onChange={(v) => void set("theme", v)} />
      <Field label="Accent color">
        <div className="flex gap-1.5">
          {ACCENTS.map((c) => (
            <button key={c} onClick={() => void set("accent", c)} className="flex h-6 w-6 items-center justify-center rounded-full" style={{ background: c, boxShadow: s.accent === c ? `0 0 0 2px rgb(var(--panel)), 0 0 0 4px ${c}` : undefined }} aria-label={c}>
              {s.accent === c && <Check className="h-3.5 w-3.5 text-white" />}
            </button>
          ))}
        </div>
      </Field>
      <Toggle k="animations" label="Animations" hint="Turn off to reduce motion" />
    </>
  );
}

function LogsSection() {
  const [text, setText] = useState("");
  const ref = useRef<HTMLPreElement>(null);
  const load = async () => {
    setText(await api.readLogs(400).catch(() => ""));
    requestAnimationFrame(() => ref.current && (ref.current.scrollTop = ref.current.scrollHeight));
  };
  useEffect(() => {
    void load();
  }, []);
  return (
    <div className="flex h-full flex-col gap-3 py-3">
      <div className="flex items-center gap-2">
        <div className="flex-1 text-[11px] text-faint">Cookies, tokens and passwords are masked before anything is written to the log.</div>
        <Button size="sm" variant="ghost" onClick={load}>
          <RefreshCw className="h-3.5 w-3.5" />
          Refresh
        </Button>
        <Button size="sm" onClick={() => void api.openLogFolder()}>
          <FolderOpen className="h-3.5 w-3.5" />
          Open folder
        </Button>
        <Button
          size="sm"
          variant="primary"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(await api.diagnostics());
              toast({ kind: "success", title: "Diagnostics copied", detail: "Paste them into a bug report." });
            } catch (e) {
              toast({ kind: "error", title: "Could not copy diagnostics", detail: errorMessage(e) });
            }
          }}
        >
          <Copy className="h-3.5 w-3.5" />
          Copy diagnostics
        </Button>
      </div>
      <pre ref={ref} className="min-h-[300px] flex-1 select-text overflow-auto whitespace-pre-wrap break-all rounded-lg border border-line bg-bg p-3 font-mono text-[11px] leading-relaxed text-muted">
        {text || "The log is empty."}
      </pre>
    </div>
  );
}


export function SettingsDialog() {
  const open = useUi((s) => s.dialog === "settings");
  const section = useUi((s) => s.settingsSection);
  const current = SECTIONS.some(([id]) => id === section) ? section : "general";
  return (
    <Dialog open={open} onOpenChange={(o) => !o && useUi.getState().openDialog(null)} title="Settings" className="h-[min(640px,88vh)] w-[min(840px,94vw)]">
      <div className="-mx-5 -my-4 flex h-full min-h-[480px]">
        <nav className="w-44 shrink-0 space-y-0.5 border-r border-line p-2">
          {SECTIONS.map(([id, label]) => (
            <button
              key={id}
              onClick={() => useUi.setState({ settingsSection: id })}
              className={cn("block h-8 w-full rounded-md px-3 text-left text-[13px]", current === id ? "bg-accent/15 text-fg" : "text-muted hover:bg-raised hover:text-fg")}
            >
              {label}
            </button>
          ))}
        </nav>
        <div className="min-w-0 flex-1 overflow-y-auto px-6 py-2">
          {current === "general" && <GeneralSection />}
          {current === "downloads" && <DownloadsSection />}
          {current === "categories" && <CategoriesSection />}
          {current === "media" && <MediaSection />}
          {current === "tools" && <ToolsSection />}
          {current === "appearance" && <AppearanceSection />}
          {current === "logs" && <LogsSection />}
          {current === "about" && <AboutSection />}
        </div>
      </div>
    </Dialog>
  );
}
