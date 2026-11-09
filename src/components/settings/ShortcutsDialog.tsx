import { useUi } from "../../store/ui";
import { Dialog } from "../ui/overlays";
import { Kbd } from "../ui/primitives";

const GROUPS: [string, [string, string][]][] = [
  [
    "Adding",
    [
      ["Ctrl+L / Ctrl+N", "Focus the link box"],
      ["Ctrl+V (in the link box)", "Paste and add immediately"],
      ["Enter (in the link box)", "Add the typed links"],
    ],
  ],
  [
    "Downloads",
    [
      ["↑ / ↓", "Move between downloads"],
      ["Shift+↑ / Shift+↓", "Extend the selection"],
      ["Ctrl+A", "Select all in this view"],
      ["Space", "Pause or resume"],
      ["Enter", "Open the finished file"],
      ["Delete", "Remove from the list"],
      ["Shift+Delete", "Delete with files"],
      ["Escape", "Clear the selection"],
    ],
  ],
  [
    "App",
    [
      ["Ctrl+B", "Show or hide the sidebar"],
      ["Ctrl+,", "Settings"],
      ["?", "This list"],
    ],
  ],
];

export function ShortcutsDialog() {
  const open = useUi((s) => s.dialog === "shortcuts");
  return (
    <Dialog open={open} onOpenChange={(o) => !o && useUi.getState().openDialog(null)} title="Keyboard shortcuts">
      <div className="space-y-5">
        {GROUPS.map(([title, rows]) => (
          <div key={title}>
            <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-faint">{title}</div>
            {rows.map(([k, d]) => (
              <div key={k} className="flex items-center justify-between border-b border-line/50 py-1.5 text-[13px] last:border-0">
                <span className="text-muted">{d}</span>
                <Kbd>{k}</Kbd>
              </div>
            ))}
          </div>
        ))}
      </div>
    </Dialog>
  );
}
