import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import appIcon from "../../assets/endownloader-icon.svg";
import { api, errorMessage } from "../../lib/api";
import { formatBytes } from "../../lib/format";
import { useUpdate } from "../../store/update";
import { Dialog } from "../ui/overlays";
import { Button } from "../ui/primitives";

type Phase = "idle" | "downloading" | "installing" | "installed" | "exiting" | "error";

export function UpdateDialog() {
  const { available: info, dialogOpen, close } = useUpdate();
  const [version, setVersion] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  useEffect(() => {
    const un = listen<{ done: number; total: number }>("update-progress", (e) => {
      setProgress(e.payload);
      if (e.payload.total && e.payload.done >= e.payload.total) setPhase("installing");
    });
    return () => void un.then((f) => f());
  }, []);

  if (!info) return null;
  const busy = phase === "downloading" || phase === "installing" || phase === "exiting";

  const start = async () => {
    if (!info.installable) {
      await api.openReleasePage().catch(() => {});
      close();
      return;
    }
    setError("");
    setProgress(null);
    setPhase("downloading");
    try {
      const r = await api.installUpdate(info);
      setPhase(r === "exiting" ? "exiting" : "installed");
    } catch (e) {
      setError(errorMessage(e));
      setPhase("error");
    }
  };

  const pct = progress && progress.total ? Math.min(100, (progress.done / progress.total) * 100) : null;

  return (
    <Dialog open={dialogOpen} onOpenChange={(o) => !o && !busy && close()} title="Update available" className="w-[min(460px,94vw)]">
      <div className="space-y-4 p-5">
        <div className="flex items-center gap-3.5">
          <img src={appIcon} alt="" className="h-14 w-14" draggable={false} />
          <div>
            <div className="text-[16px] font-semibold">ENDownloader {info.version} is available</div>
            <div className="text-[12px] text-faint">You have version {version}.</div>
          </div>
        </div>
        {info.notes && (
          <div className="max-h-52 overflow-y-auto whitespace-pre-wrap rounded-lg bg-raised p-3 text-[12px] leading-relaxed text-muted">{info.notes}</div>
        )}
        {(phase === "downloading" || phase === "installing") && (
          <div className="space-y-1.5">
            <div className="h-1.5 overflow-hidden rounded-full bg-raised">
              <div className={pct === null || phase === "installing" ? "h-full w-1/3 animate-pulse rounded-full bg-accent" : "h-full rounded-full bg-accent transition-[width]"} style={pct !== null && phase !== "installing" ? { width: `${pct}%` } : undefined} />
            </div>
            <div className="text-[11px] text-faint">
              {phase === "installing"
                ? "Installing… enter your password if Ubuntu asks for it."
                : progress
                  ? `Downloading… ${formatBytes(progress.done)}${progress.total ? ` of ${formatBytes(progress.total)}` : ""}`
                  : "Downloading…"}
            </div>
          </div>
        )}
        <div className="text-[12px] text-faint">
          {phase === "error" && <span className="text-red-400">{error}</span>}
          {phase === "installed" && `ENDownloader ${info.version} is installed. Restart to use it.`}
          {phase === "exiting" && "Installing… ENDownloader will restart by itself."}
          {phase === "idle" &&
            (info.installable
              ? "The update downloads and installs in place; your downloads, queue and settings are kept. Running downloads continue after the restart."
              : "This copy was not installed from a package, so it cannot update itself. Download the new version from the release page.")}
        </div>
        <div className="flex justify-end gap-2">
          {phase === "installed" ? (
            <>
              <Button onClick={close}>Restart later</Button>
              <Button variant="primary" onClick={() => void api.restartApp()}>Restart now</Button>
            </>
          ) : (
            <>
              <Button onClick={close} disabled={busy}>Later</Button>
              <Button variant="primary" onClick={start} disabled={busy}>
                {phase === "error" ? "Try again" : info.installable ? "Update now" : "Open download page"}
              </Button>
            </>
          )}
        </div>
      </div>
    </Dialog>
  );
}
