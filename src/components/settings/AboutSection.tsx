import { getVersion } from "@tauri-apps/api/app";
import { Copy, ExternalLink, FolderOpen } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import appIcon from "../../assets/endownloader-icon.svg";
import { api, errorMessage } from "../../lib/api";
import type { AppPaths, ToolInfo } from "../../lib/types";
import { toast } from "../../store/ui";
import { Button } from "../ui/primitives";

type About = Awaited<ReturnType<typeof api.aboutInfo>>;

const PLATFORM: Record<string, string> = { linux: "Linux", windows: "Windows", macos: "macOS" };
const TOOL_NAME: Record<string, string> = { "yt-dlp": "yt-dlp", ffmpeg: "FFmpeg", deno: "Deno", node: "Node.js", "gallery-dl": "gallery-dl" };
const LIBRARIES: [string, string, string][] = [
  ["Tauri", "desktop shell", "MIT / Apache-2.0"],
  ["aria2", "multi-connection downloads", "GPL-2.0, separate program"],
  ["yt-dlp", "media sites", "Unlicense, separate program"],
  ["FFmpeg", "joining video and audio, MP3", "GPL / LGPL, separate program"],
  ["Rust · Tokio · reqwest", "engine", "MIT / Apache-2.0"],
  ["SQLite", "queue and history", "public domain"],
  ["React · Radix UI · zustand", "interface", "MIT"],
  ["Tailwind CSS · Lucide", "styling · icons", "MIT · ISC"],
];

const Section = ({ title, children }: { title: string; children: ReactNode }) => (
  <section>
    <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-faint">{title}</div>
    <div className="divide-y divide-line rounded-lg bg-raised px-3">{children}</div>
  </section>
);
const Row = ({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children?: ReactNode }) => (
  <div className="flex items-center justify-between gap-4 py-2.5">
    <span className="min-w-0">
      {label}
      {hint && <span className="block truncate text-[11px] text-faint">{hint}</span>}
    </span>
    {children}
  </div>
);

export function AboutSection() {
  const [version, setVersion] = useState("");
  const [about, setAbout] = useState<About | null>(null);
  const [paths, setPaths] = useState<AppPaths | null>(null);
  const [tools, setTools] = useState<ToolInfo[]>([]);
  useEffect(() => {
    getVersion().then(setVersion, () => {});
    api.aboutInfo().then(setAbout, () => {});
    api.appPaths().then(setPaths, () => {});
    api.toolsStatus().then(setTools, () => {});
  }, []);

  const fail = (what: string) => (e: unknown) => toast({ kind: "error", title: `Could not open ${what}`, detail: errorMessage(e) });
  const copyDiagnostics = async () => {
    try {
      const text = await api.diagnostics();
      const extra = about ? `\nBuild: ${about.build} · Tauri ${about.tauriVersion} · WebView ${about.webviewVersion || "unknown"}` : "";
      await navigator.clipboard.writeText(text + extra);
      toast({ kind: "success", title: "Diagnostics copied", detail: "Paste them into your bug report (secrets are masked)" });
    } catch (e) {
      toast({ kind: "error", title: "Could not collect diagnostics", detail: errorMessage(e) });
    }
  };
  const found = tools.filter((t) => t.path);
  const mono = "font-mono text-[12px] text-muted";

  return (
    <div className="space-y-5 py-4 text-[13px]">
      <div className="flex items-center gap-4">
        <img src={appIcon} alt="" className="h-16 w-16" draggable={false} />
        <div>
          <div className="text-lg font-semibold">
            ENDownloader <span className="font-normal text-muted">{version}</span>
          </div>
          <div className="text-muted">A fast download manager for files and media.</div>
          <div className="mt-1 text-[11px] text-faint">Parallel downloads that survive restarts, and media from YouTube, TikTok, Instagram and more.</div>
        </div>
      </div>

      <div>
        <Section title="This copy">
          <Row label="Version">
            <span className={mono}>
              {version} {about?.build === "debug" && <span className="ml-1 rounded bg-amber-500/20 px-1 text-amber-500">debug build</span>}
              {paths?.portable && <span className="ml-1 rounded bg-accent/20 px-1 text-accent">portable</span>}
            </span>
          </Row>
          <Row label="System">
            <span className="text-muted">{about ? `${PLATFORM[about.platform] ?? about.platform} · ${about.arch}` : "…"}</span>
          </Row>
          <Row label="Tauri · WebView">
            <span className={mono}>{about ? `${about.tauriVersion} · ${about.webviewVersion || "unknown"}` : "…"}</span>
          </Row>
          <Row label="Helper tools" hint="Settings → Tools">
            <span className="text-right text-[12px] text-muted">{found.length ? found.map((t) => `${TOOL_NAME[t.id] ?? t.id} ${t.version ?? ""}`.trim()).join(" · ") : "…"}</span>
          </Row>
          <Row label="Settings, queue and history" hint={paths?.data}>
            <Button size="sm" disabled={!paths} onClick={() => paths && api.openPath(paths.data).catch(fail("the data folder"))}>
              <FolderOpen className="h-3.5 w-3.5" />
              Open
            </Button>
          </Row>
          <Row label="Logs" hint={paths?.logs}>
            <Button size="sm" onClick={() => api.openLogFolder().catch(fail("the logs folder"))}>
              <FolderOpen className="h-3.5 w-3.5" />
              Open
            </Button>
          </Row>
        </Section>
        <div className="mt-2 flex justify-end">
          <Button size="sm" variant="ghost" onClick={() => void copyDiagnostics()}>
            <Copy className="h-3.5 w-3.5" />
            Copy diagnostics
          </Button>
        </div>
      </div>

      <Section title="Developer">
        <Row label="Developed by">
          <span className="text-muted">M. Estiaque Ahmed Khan</span>
        </Row>
        <Row label="Contact" hint="Bug reports, ideas and feedback are welcome">
          <button className="text-accent hover:underline" onClick={() => api.openUrl("mailto:info@mestiaque.com").catch(() => toast({ kind: "error", title: "No mail app found", detail: "info@mestiaque.com" }))}>
            info@mestiaque.com
          </button>
        </Row>
        <Row label="Website">
          <button className="flex items-center gap-1 text-accent hover:underline" onClick={() => api.openUrl("https://mestiaque.com").catch(fail("the website"))}>
            mestiaque.com
            <ExternalLink className="h-3 w-3" />
          </button>
        </Row>
        <Row label="Also by the developer" hint="Same look and themes">
          <span className="text-muted">ENViewer · ENPlayer · ENPost</span>
        </Row>
        <Row label="Licence">
          <span className="text-muted">© 2026 M. Estiaque Ahmed Khan · MIT</span>
        </Row>
      </Section>

      <Section title="Built with">
        {LIBRARIES.map(([name, what, licence]) => (
          <div key={name} className="flex items-center justify-between gap-4 py-2 text-[12px]">
            <span>
              {name} <span className="text-faint">· {what}</span>
            </span>
            <span className="text-faint">{licence}</span>
          </div>
        ))}
      </Section>
    </div>
  );
}
