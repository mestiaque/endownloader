import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { CheckCheck, ClipboardPaste, FileText, Keyboard, Link2, Monitor, Moon, MoreHorizontal, PanelLeft, Pause, Play, Settings, Sparkles, Sun, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { useSettings } from "../../store/settings";
import { errorMessage } from "../../lib/api";
import { addLinks, importLinksFile } from "../../store/tasks";
import { toast, useUi } from "../../store/ui";
import { DropdownMenu } from "../ui/menus";
import { Button, IconButton } from "../ui/primitives";
import { WindowControls, windowDragProps } from "../ui/WindowControls";

/** Link box: Enter adds what is typed; Ctrl+V (paste) adds immediately. */
function LinkBox() {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLInputElement>(null);

  const submit = async (text: string) => {
    if (!text.trim() || busy) return;
    setBusy(true);
    const added = await addLinks(text);
    setBusy(false);
    if (added > 0) setValue("");
  };

  const pasteAndAnalyze = async () => {
    try {
      const text = await readText();
      if (!text?.trim()) return toast({ kind: "info", title: "The clipboard is empty" });
      await submit(text);
    } catch {
      toast({ kind: "error", title: "Could not read the clipboard" });
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === "l" || e.key.toLowerCase() === "n")) {
        e.preventDefault();
        ref.current?.focus();
        ref.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex min-w-0 flex-1 items-center gap-1.5" data-no-drag>
      <div className="relative min-w-0 flex-1">
        <Link2 className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-faint" />
        <input
          ref={ref}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onPaste={(e) => {
            const pasted = e.clipboardData.getData("text");
            if (!pasted.trim()) return;
            e.preventDefault();
            void submit(value ? `${value} ${pasted}` : pasted);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit(value);
            if (e.key === "Escape") {
              setValue("");
              (e.target as HTMLInputElement).blur();
            }
            e.stopPropagation();
          }}
          placeholder="Paste links here — several at once are fine"
          aria-label="Links to download"
          className="h-8 w-full rounded-md border border-line bg-bg pl-8 pr-7 text-[13px] outline-none placeholder:text-faint focus:border-accent"
        />
        {value && (
          <button className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-faint hover:text-fg" onClick={() => setValue("")} aria-label="Clear">
            <X className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
      <Button variant="primary" onClick={pasteAndAnalyze} disabled={busy} title="Paste links from the clipboard and add them (Ctrl+V in the box does the same)">
        <ClipboardPaste className="h-3.5 w-3.5" />
        Paste &amp; Analyze
      </Button>
    </div>
  );
}

async function importFromFile() {
  const path = await open({ multiple: false, directory: false, title: "Import links from a text file", filters: [{ name: "Text files", extensions: ["txt", "list", "csv", "md", "html", "htm"] }, { name: "All files", extensions: ["*"] }] });
  if (typeof path === "string") await importLinksFile(path);
}

async function clearCompleted() {
  try {
    const n = await api.clearCompleted();
    toast({ kind: "info", title: n ? `Cleared ${n} completed download${n > 1 ? "s" : ""}` : "Nothing to clear", detail: n ? "The files stay on disk." : undefined });
  } catch (e) {
    toast({ kind: "error", title: "Could not clear", detail: errorMessage(e) });
  }
}

export function Toolbar() {
  const { settings, set } = useSettings();
  const themeIcon =
    settings.theme === "light" ? <Sun className="h-4 w-4" /> : settings.theme === "dark" ? <Moon className="h-4 w-4" /> : settings.theme === "glass" ? <Sparkles className="h-4 w-4" /> : <Monitor className="h-4 w-4" />;

  return (
    <header className="flex h-12 shrink-0 items-center gap-1 border-b border-line bg-panel/80 px-2 backdrop-blur" {...windowDragProps}>
      <IconButton label={settings.sidebarCollapsed ? "Show sidebar" : "Hide sidebar"} shortcut="Ctrl+B" onClick={() => set("sidebarCollapsed", !settings.sidebarCollapsed)}>
        <PanelLeft className="h-4 w-4" />
      </IconButton>
      <div className="mx-1 flex min-w-0 flex-1">
        <LinkBox />
      </div>
      <IconButton label="Resume all" onClick={() => void api.resumeAll()}>
        <Play className="h-4 w-4" />
      </IconButton>
      <IconButton label="Pause all" onClick={() => void api.pauseAll()}>
        <Pause className="h-4 w-4" />
      </IconButton>
      <div className="mx-1 h-5 w-px bg-line" />
      <DropdownMenu
        trigger={<IconButton label="Theme">{themeIcon}</IconButton>}
        entries={[
          { label: "Dark", icon: <Moon className="h-3.5 w-3.5" />, checked: settings.theme === "dark", onSelect: () => set("theme", "dark") },
          { label: "Light", icon: <Sun className="h-3.5 w-3.5" />, checked: settings.theme === "light", onSelect: () => set("theme", "light") },
          { label: "Glass", icon: <Sparkles className="h-3.5 w-3.5" />, checked: settings.theme === "glass", onSelect: () => set("theme", "glass") },
          { label: "System", icon: <Monitor className="h-3.5 w-3.5" />, checked: settings.theme === "system", onSelect: () => set("theme", "system") },
        ]}
      />
      <DropdownMenu
        trigger={
          <IconButton label="More">
            <MoreHorizontal className="h-4 w-4" />
          </IconButton>
        }
        entries={[
          { label: "Import links from file…", icon: <FileText className="h-3.5 w-3.5" />, onSelect: () => void importFromFile() },
          { label: "Clear completed", icon: <CheckCheck className="h-3.5 w-3.5" />, onSelect: () => void clearCompleted() },
          { separator: true },
          { label: "Keyboard shortcuts", icon: <Keyboard className="h-3.5 w-3.5" />, shortcut: "?", onSelect: () => useUi.getState().openDialog("shortcuts") },
          { separator: true },
          { label: "Hide to tray", onSelect: () => void api.hideToTray() },
          { label: "Quit ENDownloader", danger: true, onSelect: () => void api.quitApp() },
        ]}
      />
      <IconButton label="Settings" shortcut="Ctrl+," onClick={() => useUi.getState().openDialog("settings")}>
        <Settings className="h-4 w-4" />
      </IconButton>
      <WindowControls />
    </header>
  );
}
