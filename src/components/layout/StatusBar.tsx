import { ArrowDown } from "lucide-react";
import { useMemo } from "react";
import { formatBytes, formatSpeed } from "../../lib/format";
import { useSettings } from "../../store/settings";
import { useTasks, useVisibleTasks } from "../../store/tasks";

export function StatusBar() {
  const tasks = useTasks((s) => s.tasks);
  const selection = useTasks((s) => s.selection);
  const byId = useTasks((s) => s.byId);
  const visible = useVisibleTasks();
  const maxParallel = useSettings((s) => s.settings.maxParallel);
  const { active, queued, speed } = useMemo(() => {
    let active = 0,
      queued = 0,
      speed = 0;
    for (const t of tasks) {
      if (t.status === "active" || t.status === "starting") {
        active++;
        speed += t.speed;
      } else if (t.status === "queued") queued++;
    }
    return { active, queued, speed };
  }, [tasks]);
  const selectedBytes = useMemo(() => [...selection].reduce((n, id) => n + (byId.get(id)?.total ?? 0), 0), [selection, byId]);

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-line bg-panel px-3 text-[11px] text-muted">
      <span>
        {visible.length === tasks.length ? `${tasks.length.toLocaleString()} downloads` : `${visible.length.toLocaleString()} of ${tasks.length.toLocaleString()} downloads`}
      </span>
      {selection.size > 0 && (
        <span className="text-fg">
          {selection.size.toLocaleString()} selected{selectedBytes > 0 ? ` · ${formatBytes(selectedBytes)}` : ""}
        </span>
      )}
      <span className="flex-1" />
      <span>
        {active} of {maxParallel} running · {queued} queued
      </span>
      <span className="flex items-center gap-1 tabular-nums text-fg">
        <ArrowDown className="h-3 w-3 text-accent" />
        {formatSpeed(speed)}
      </span>
    </footer>
  );
}
