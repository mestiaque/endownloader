import { describe, expect, it } from "vitest";
import type { Task } from "../lib/types";
import { batchSummary } from "./batch";

const t = (id: number, status: Task["status"], total: number | null, done: number, extra: Partial<Task> = {}): Task => ({
  id, url: `https://x/${id}`, engine: "aria2", status, title: null, filename: `f${id}`, folder: "/d", category: "other",
  total, done, speed: 0, connections: 8, error: null, errorKind: null, priority: 0, position: id,
  createdAt: 1000 + id, finishedAt: null, options: null, ...extra,
});

describe("batchSummary", () => {
  it("counts this round: finished since the oldest unfinished task, plus the unfinished ones", () => {
    const tasks = [
      t(1, "completed", 100, 100, { createdAt: 10, finishedAt: 20 }), // an older round: not counted
      t(2, "completed", 200, 200, { finishedAt: 2000 }),
      t(3, "active", 400, 100, { speed: 50 }),
      t(4, "queued", 300, 0),
      t(5, "queued", 100, 0),
    ];
    const s = batchSummary(tasks);
    expect(s.count).toBe(4);
    expect(s.finished).toBe(1);
    expect(s.total).toBe(1000);
    expect(s.done).toBe(300);
    expect(s.speed).toBe(50);
    expect(s.eta).toBe(14);
    expect(s.current?.id).toBe(3);
  });

  it("ETA counts known sizes and flags unknown ones; paused when everything is paused", () => {
    const s = batchSummary([t(1, "active", 100, 50, { speed: 10 }), t(2, "queued", null, 0)]);
    expect(s.eta).toBe(5);
    expect(s.unknownSize).toBe(true);
    expect(batchSummary([t(1, "paused", 10, 5), t(2, "paused", 10, 0)]).paused).toBe(true);
  });

  it("nothing open", () => {
    const s = batchSummary([t(1, "completed", 10, 10, { finishedAt: 5 })]);
    expect(s.open).toBe(0);
    expect(s.count).toBe(0);
  });
});
