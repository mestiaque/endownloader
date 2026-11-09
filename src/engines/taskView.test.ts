import { describe, expect, it } from "vitest";
import type { Task } from "../lib/types";
import { countViews, etaSeconds, inView, matchesSearch, percent } from "./taskView";

const t = (p: Partial<Task>): Task => ({
  id: 1,
  url: "https://example.com/a.iso",
  engine: "auto",
  status: "queued",
  title: null,
  filename: "a.iso",
  folder: "/tmp",
  category: "iso",
  total: null,
  done: 0,
  speed: 0,
  connections: 8,
  error: null,
  errorKind: null,
  priority: 0,
  position: 1,
  createdAt: 0,
  finishedAt: null,
  options: null,
  ...p,
});

describe("task views", () => {
  it("filters by status and category", () => {
    expect(inView(t({ status: "paused" }), "queued")).toBe(true);
    expect(inView(t({ status: "active" }), "active")).toBe(true);
    expect(inView(t({ status: "cancelled" }), "failed")).toBe(false);
    expect(inView(t({ status: "cancelled" }), "all")).toBe(true);
    expect(inView(t({ category: "iso" }), "cat:iso")).toBe(true);
    expect(inView(t({ category: "video" }), "cat:iso")).toBe(false);
  });

  it("counts", () => {
    const c = countViews([t({ status: "active" }), t({ status: "failed", category: "video" }), t({ status: "completed" })], ["iso", "video"]);
    expect(c).toMatchObject({ all: 3, active: 1, failed: 1, completed: 1, queued: 0, "cat:iso": 2, "cat:video": 1 });
  });

  it("search", () => {
    expect(matchesSearch(t({ title: "Ubuntu Desktop" }), "desk")).toBe(true);
    expect(matchesSearch(t({}), "example.com")).toBe(true);
    expect(matchesSearch(t({}), "zzz")).toBe(false);
  });
});

describe("progress", () => {
  it("never fakes a percentage", () => {
    expect(percent(t({ done: 500, total: null }))).toBeNull();
    expect(percent(t({ done: 500, total: 0 }))).toBeNull();
    expect(percent(t({ done: 250, total: 1000 }))).toBe(25);
    expect(percent(t({ status: "completed", total: null }))).toBe(100);
  });
  it("eta", () => {
    expect(etaSeconds(t({ done: 0, total: 1000, speed: 100 }))).toBe(10);
    expect(etaSeconds(t({ done: 0, total: null, speed: 100 }))).toBeNull();
    expect(etaSeconds(t({ done: 0, total: 1000, speed: 0 }))).toBeNull();
  });
});
