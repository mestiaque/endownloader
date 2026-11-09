import * as Tooltip from "@radix-ui/react-tooltip";
import { listen } from "@tauri-apps/api/event";
import { Link2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Sidebar } from "./components/layout/Sidebar";
import { StatusBar } from "./components/layout/StatusBar";
import { Toolbar } from "./components/layout/Toolbar";
import { SettingsDialog } from "./components/settings/SettingsDialog";
import { ShortcutsDialog } from "./components/settings/ShortcutsDialog";
import { TaskList } from "./components/tasks/TaskList";
import { ConfirmHost, PromptHost, Toaster } from "./components/ui/overlays";
import { canPause, canResume } from "./engines/taskView";
import { api } from "./lib/api";
import type { CliRequest } from "./lib/types";
import { useSettings } from "./store/settings";
import { addLinks, removeTasks, runAction, targetTasks, useTasks, visibleSnapshot } from "./store/tasks";
import { toast, useUi } from "./store/ui";

function hexToRgb(hex: string): string {
  const m = hex.replace("#", "").match(/^([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i);
  return m ? `${parseInt(m[1], 16)} ${parseInt(m[2], 16)} ${parseInt(m[3], 16)}` : "59 130 246";
}

function useTheme() {
  const theme = useSettings((s) => s.settings.theme);
  const accent = useSettings((s) => s.settings.accent);
  const animations = useSettings((s) => s.settings.animations);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || theme === "glass" || (theme === "system" && mq.matches);
      document.documentElement.classList.toggle("dark", dark);
      document.documentElement.classList.toggle("light", !dark);
      document.documentElement.classList.toggle("glass", theme === "glass");
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    document.documentElement.style.setProperty("--accent", hexToRgb(accent));
  }, [accent]);
  useEffect(() => {
    document.documentElement.classList.toggle("reduce-motion", !animations);
  }, [animations]);
}

function filePath(t: { folder: string; filename: string | null }) {
  const sep = t.folder.includes("\\") ? "\\" : "/";
  return t.filename ? `${t.folder.replace(/[\\/]$/, "")}${sep}${t.filename}` : t.folder;
}

function useKeys() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ui = useUi.getState();
      const t = e.target as HTMLElement;
      const ctrl = e.ctrlKey || e.metaKey;
      const key = e.key;
      if (ctrl && key === ",") {
        e.preventDefault();
        return ui.openDialog("settings");
      }
      if (ui.dialog || ui.confirmReq || ui.promptReq) return;
      if (e.defaultPrevented || t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable) return;
      if (t.closest?.('[role="menu"],[role="dialog"],[role="listbox"]')) return;
      const store = useTasks.getState();
      const settings = useSettings.getState();
      const move = (delta: number, extend: boolean) => {
        const v = visibleSnapshot();
        if (!v.length) return;
        const cur = v.findIndex((x) => x.id === store.focused);
        const next = cur < 0 ? 0 : Math.min(v.length - 1, Math.max(0, cur + delta));
        if (extend) store.select(v[next].id, "range", v);
        else store.select(v[next].id, "single", v);
        useTasks.setState({ focused: v[next].id });
      };
      let handled = true;
      if (ctrl && key.toLowerCase() === "b") void settings.set("sidebarCollapsed", !settings.settings.sidebarCollapsed);
      else if (key === "?") ui.openDialog("shortcuts");
      else if (ctrl && key.toLowerCase() === "a") store.setSelection(visibleSnapshot().map((x) => x.id));
      else if (key === "Escape") store.clearSelection();
      else if (key === "ArrowDown") move(1, e.shiftKey);
      else if (key === "ArrowUp") move(-1, e.shiftKey);
      else if (key === "Home") move(-Infinity, e.shiftKey);
      else if (key === "End") move(Infinity, e.shiftKey);
      else if (key === "PageDown") move(10, e.shiftKey);
      else if (key === "PageUp") move(-10, e.shiftKey);
      else if (key === " ") {
        const ts = targetTasks();
        if (ts.some(canPause)) void runAction(ts.filter(canPause), "pause");
        else void runAction(ts.filter(canResume), "resume");
      } else if (key === "Enter") {
        const f = targetTasks()[0];
        if (f?.status === "completed") api.openPath(filePath(f)).catch(() => toast({ kind: "error", title: "Could not open the file" }));
      } else if (key === "Delete") void removeTasks(targetTasks(), e.shiftKey);
      else handled = false;
      if (handled) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/** Drop links (from a browser or text) or a .txt link list anywhere on the window. */
function useDropLinks(): boolean {
  const [over, setOver] = useState(false);
  useEffect(() => {
    let depth = 0;
    const accepts = (e: DragEvent) => !!e.dataTransfer && [...e.dataTransfer.types].some((t) => t === "Files" || t === "text/uri-list" || t === "text/plain");
    const enter = (e: DragEvent) => {
      if (!accepts(e)) return;
      e.preventDefault();
      depth++;
      setOver(true);
    };
    const leave = () => {
      depth = Math.max(0, depth - 1);
      if (!depth) setOver(false);
    };
    const overFn = (e: DragEvent) => {
      if (!accepts(e)) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    };
    const drop = async (e: DragEvent) => {
      depth = 0;
      setOver(false);
      const dt = e.dataTransfer;
      if (!dt) return;
      e.preventDefault();
      const parts: string[] = [];
      for (const f of [...dt.files]) {
        if (f.size > 20 * 1024 * 1024) {
          toast({ kind: "error", title: `${f.name} is too large to be a link list` });
          continue;
        }
        if (f.type.startsWith("text/") || /\.(txt|list|csv|md|html?)$/i.test(f.name)) parts.push(await f.text());
        else toast({ kind: "info", title: `${f.name} is not a link list`, detail: "Drop links, or a .txt file with links." });
      }
      if (!dt.files.length) parts.push(dt.getData("text/uri-list") || dt.getData("text/plain"));
      const text = parts.join("\n");
      if (text.trim()) void addLinks(text);
    };
    window.addEventListener("dragenter", enter);
    window.addEventListener("dragleave", leave);
    window.addEventListener("dragover", overFn);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragenter", enter);
      window.removeEventListener("dragleave", leave);
      window.removeEventListener("dragover", overFn);
      window.removeEventListener("drop", drop);
    };
  }, []);
  return over;
}

function handleCli(req: CliRequest | null) {
  if (req?.urls.length) void addLinks(req.urls.join("\n"), { folder: req.folder, quality: req.quality });
}

export default function App() {
  const [ready, setReady] = useState(false);
  useTheme();
  useKeys();
  const dropping = useDropLinks();

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    (async () => {
      await useSettings.getState().load();
      cleanup = await useTasks
        .getState()
        .init()
        .catch((e) => {
          console.error(e);
          return undefined;
        });
      setReady(true);
      handleCli(await api.getLaunchRequest().catch(() => null));
    })();
    const un = listen<CliRequest>("cli-request", (e) => handleCli(e.payload));
    const onContext = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName !== "INPUT" && t.tagName !== "TEXTAREA" && !t.closest("pre")) e.preventDefault();
    };
    window.addEventListener("contextmenu", onContext);
    return () => {
      cleanup?.();
      un.then((f) => f());
      window.removeEventListener("contextmenu", onContext);
    };
  }, []);

  if (!ready) return <div className="h-full bg-bg" />;

  return (
    <Tooltip.Provider>
      <div className="flex h-full">
        <Sidebar />
        <main className="flex min-w-0 flex-1 flex-col">
          <Toolbar />
          <TaskList />
          <StatusBar />
        </main>
      </div>
      {dropping && (
        <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-bg/70 backdrop-blur-sm">
          <div className="flex flex-col items-center gap-2 rounded-xl border-2 border-dashed border-accent bg-panel px-10 py-8 text-center shadow-xl">
            <Link2 className="h-7 w-7 text-accent" />
            <div className="text-[14px] font-medium">Drop to add downloads</div>
            <div className="text-[12px] text-muted">Links, or a text file with links</div>
          </div>
        </div>
      )}
      <SettingsDialog />
      <ShortcutsDialog />
      <ConfirmHost />
      <PromptHost />
      <Toaster />
    </Tooltip.Provider>
  );
}
