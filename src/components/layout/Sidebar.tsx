import { CheckCircle2, CircleAlert, Clock, Download, Layers } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { countViews, type ViewId } from "../../engines/taskView";
import { cn } from "../../lib/format";
import { useSettings } from "../../store/settings";
import { useTasks } from "../../store/tasks";
import { Tip } from "../ui/primitives";
import { CategoryIcon } from "../tasks/categoryIcon";
import appIcon from "../../assets/endownloader-icon.svg";

function Row({ icon, label, active, onClick, collapsed, count, tone }: { icon: ReactNode; label: string; active?: boolean; onClick: () => void; collapsed: boolean; count?: number; tone?: "danger" }) {
  const btn = (
    <button
      onClick={onClick}
      className={cn(
        "group flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-[13px] transition-colors",
        active ? "bg-accent/15 text-fg" : "text-muted hover:bg-raised hover:text-fg",
        collapsed && "justify-center px-0",
      )}
    >
      <span className={cn("shrink-0", active && "text-accent")}>{icon}</span>
      {!collapsed && <span className="min-w-0 flex-1 truncate">{label}</span>}
      {!collapsed && count != null && count > 0 && <span className={cn("text-[11px] tabular-nums", tone === "danger" ? "text-danger" : "text-faint")}>{count.toLocaleString()}</span>}
    </button>
  );
  return collapsed ? (
    <Tip label={count ? `${label} (${count})` : label} side="right">
      {btn}
    </Tip>
  ) : (
    btn
  );
}

function Section({ title, children, collapsed }: { title: string; children: ReactNode; collapsed: boolean }) {
  return (
    <div className="mt-4 first:mt-1">
      {!collapsed ? (
        <div className="mb-1 flex h-6 items-center px-2 text-[11px] font-semibold uppercase tracking-wider text-faint">{title}</div>
      ) : (
        <div className="mx-3 mb-2 h-px bg-line" />
      )}
      <div className="space-y-0.5">{children}</div>
    </div>
  );
}

const STATUS_ROWS: { id: ViewId; label: string; icon: ReactNode }[] = [
  { id: "all", label: "All downloads", icon: <Layers className="h-4 w-4" /> },
  { id: "active", label: "Active", icon: <Download className="h-4 w-4" /> },
  { id: "queued", label: "Queued", icon: <Clock className="h-4 w-4" /> },
  { id: "completed", label: "Completed", icon: <CheckCircle2 className="h-4 w-4" /> },
  { id: "failed", label: "Failed", icon: <CircleAlert className="h-4 w-4" /> },
];

export function Sidebar() {
  const collapsed = useSettings((s) => s.settings.sidebarCollapsed);
  const categories = useSettings((s) => s.settings.categories);
  const tasks = useTasks((s) => s.tasks);
  const view = useTasks((s) => s.view);
  const setView = useTasks((s) => s.setView);
  const counts = useMemo(
    () =>
      countViews(
        tasks,
        categories.map((c) => c.id),
      ),
    [tasks, categories],
  );

  return (
    <aside className={cn("flex shrink-0 flex-col border-r border-line bg-panel transition-[width] duration-200", collapsed ? "w-[52px]" : "w-[220px]")}>
      <div className={cn("flex h-12 items-center gap-2 px-4", collapsed && "justify-center px-0")}>
        <img src={appIcon} alt="" className="h-6 w-6 shrink-0" draggable={false} />
        {!collapsed && <span className="text-[14px] font-semibold tracking-tight">ENDownloader</span>}
      </div>
      <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
        <Section title="Downloads" collapsed={collapsed}>
          {STATUS_ROWS.map((r) => (
            <Row key={r.id} collapsed={collapsed} icon={r.icon} label={r.label} active={view === r.id} count={counts[r.id]} tone={r.id === "failed" ? "danger" : undefined} onClick={() => setView(r.id)} />
          ))}
        </Section>
        <Section title="Categories" collapsed={collapsed}>
          {categories.map((c) => (
            <Row
              key={c.id}
              collapsed={collapsed}
              icon={<CategoryIcon id={c.id} />}
              label={c.name}
              active={view === `cat:${c.id}`}
              count={counts[`cat:${c.id}`]}
              onClick={() => setView(`cat:${c.id}`)}
            />
          ))}
        </Section>
      </nav>
    </aside>
  );
}
