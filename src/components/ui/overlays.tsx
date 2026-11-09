import * as D from "@radix-ui/react-dialog";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../../lib/format";
import { useUi } from "../../store/ui";
import { Button, Input, ProgressBar, Spinner } from "./primitives";

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  className,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}) {
  return (
    <D.Root open={open} onOpenChange={onOpenChange}>
      <D.Portal>
        <D.Overlay className="fixed inset-0 z-[80] bg-black/50 backdrop-blur-[2px] animate-fade-in" />
        <D.Content
          onKeyDown={(e) => e.stopPropagation()}
          className={cn(
            "fixed left-1/2 top-1/2 z-[81] flex max-h-[88vh] w-[min(560px,94vw)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-xl border border-line bg-panel shadow-pop animate-scale-in focus:outline-none",
            className,
          )}
        >
          <div className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
            <div>
              <D.Title className="text-[15px] font-semibold">{title}</D.Title>
              {description ? <D.Description className="mt-0.5 text-xs text-muted">{description}</D.Description> : <D.Description className="sr-only">{String(title)}</D.Description>}
            </div>
            <D.Close className="rounded-md p-1 text-muted hover:bg-raised hover:text-fg" aria-label="Close">
              <X className="h-4 w-4" />
            </D.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
          {footer && <div className="flex items-center justify-end gap-2 border-t border-line px-5 py-3">{footer}</div>}
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}

export function ConfirmHost() {
  const req = useUi((s) => s.confirmReq);
  const close = (ok: boolean) => {
    req?.resolve(ok);
    useUi.setState({ confirmReq: null });
  };
  return (
    <Dialog
      open={!!req}
      onOpenChange={(o) => !o && close(false)}
      title={
        <span className="flex items-center gap-2">
          {req?.danger && <AlertTriangle className="h-4 w-4 text-danger" />}
          {req?.title}
        </span>
      }
      className="w-[min(440px,94vw)]"
      footer={
        <>
          <Button onClick={() => close(false)}>Cancel</Button>
          <Button autoFocus variant={req?.danger ? "danger" : "primary"} onClick={() => close(true)}>
            {req?.confirmLabel ?? "OK"}
          </Button>
        </>
      }
    >
      <p className="whitespace-pre-line text-[13px] text-muted">{req?.message}</p>
    </Dialog>
  );
}

export function PromptHost() {
  const req = useUi((s) => s.promptReq);
  const [value, setValue] = useState("");
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!req) return;
    setValue(req.initial);
    requestAnimationFrame(() => {
      const el = ref.current;
      if (!el) return;
      el.focus();
      const dot = req.initial.lastIndexOf(".");
      if (req.selectStem && dot > 0) el.setSelectionRange(0, dot);
      else el.select();
    });
  }, [req]);
  const close = (v: string | null) => {
    req?.resolve(v);
    useUi.setState({ promptReq: null });
  };
  return (
    <Dialog
      open={!!req}
      onOpenChange={(o) => !o && close(null)}
      title={req?.title}
      className="w-[min(440px,94vw)]"
      footer={
        <>
          <Button onClick={() => close(null)}>Cancel</Button>
          <Button variant="primary" onClick={() => close(value)} disabled={!value.trim()}>
            {req?.confirmLabel ?? "OK"}
          </Button>
        </>
      }
    >
      {req?.label && <label className="mb-1.5 block text-xs text-muted">{req.label}</label>}
      <Input
        ref={ref}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && value.trim()) close(value);
        }}
      />
    </Dialog>
  );
}

const icons = {
  info: <Info className="h-4 w-4 text-accent" />,
  success: <CheckCircle2 className="h-4 w-4 text-success" />,
  error: <XCircle className="h-4 w-4 text-danger" />,
  progress: <Spinner className="h-4 w-4 text-accent" />,
};

export function Toaster() {
  const toasts = useUi((s) => s.toasts);
  const dismiss = useUi((s) => s.dismiss);
  return (
    <div className="pointer-events-none fixed bottom-10 right-4 z-[120] flex w-[340px] flex-col gap-2">
      {toasts.map((t) => (
        <div key={t.id} role="status" className="pointer-events-auto rounded-lg border border-line bg-panel p-3 shadow-pop animate-slide-up">
          <div className="flex items-start gap-2.5">
            <span className="mt-0.5">{icons[t.kind]}</span>
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium">{t.title}</div>
              {t.detail && <div className="mt-0.5 whitespace-pre-line break-words text-xs text-muted">{t.detail}</div>}
              {t.progress && <ProgressBar className="mt-2" value={t.progress.done} total={t.progress.total} />}
              {(t.onCancel || t.action) && (
                <div className="mt-2 flex gap-2">
                  {t.action && (
                    <Button size="sm" variant="subtle" onClick={t.action.run}>
                      {t.action.label}
                    </Button>
                  )}
                  {t.onCancel && t.kind === "progress" && (
                    <Button size="sm" variant="subtle" onClick={t.onCancel}>
                      Cancel
                    </Button>
                  )}
                </div>
              )}
            </div>
            {t.kind !== "progress" && (
              <button onClick={() => dismiss(t.id)} className="rounded p-0.5 text-faint hover:text-fg" aria-label="Dismiss">
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
