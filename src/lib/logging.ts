import { invoke } from "@tauri-apps/api/core";

/** Forward uncaught errors and console warnings/errors to the backend log file. */
export function installErrorForwarding() {
  const send = (level: "error" | "warn", parts: unknown[]) => {
    const message = parts.map((p) => (p instanceof Error ? `${p.message}\n${p.stack ?? ""}` : typeof p === "string" ? p : safeJson(p))).join(" ");
    invoke("frontend_log", { level, message }).catch(() => {});
  };
  for (const level of ["error", "warn"] as const) {
    const orig = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      orig(...args);
      send(level, args);
    };
  }
  window.addEventListener("error", (e) => send("error", [e.error ?? e.message]));
  window.addEventListener("unhandledrejection", (e) => send("error", ["Unhandled rejection:", e.reason]));
}

function safeJson(v: unknown) {
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}
