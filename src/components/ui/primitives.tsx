import * as RSlider from "@radix-ui/react-slider";
import * as RSwitch from "@radix-ui/react-switch";
import * as Tooltip from "@radix-ui/react-tooltip";
import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cn } from "../../lib/format";

type Variant = "default" | "primary" | "ghost" | "danger" | "subtle";

const variants: Record<Variant, string> = {
  default: "bg-raised hover:bg-line/80 text-fg border border-line",
  primary: "bg-accent text-accent-fg hover:brightness-110 border border-accent",
  ghost: "hover:bg-raised text-muted hover:text-fg",
  subtle: "bg-raised/60 hover:bg-raised text-fg",
  danger: "bg-danger text-white hover:brightness-110 border border-danger",
};

export const Button = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }>(
  ({ className, variant = "default", size = "md", ...props }, ref) => (
    <button
      ref={ref}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:pointer-events-none disabled:opacity-40",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-[13px]",
        variants[variant],
        className,
      )}
      {...props}
    />
  ),
);
Button.displayName = "Button";

export function Tip({ label, shortcut, children, side = "bottom" }: { label: ReactNode; shortcut?: string; children: ReactNode; side?: "top" | "bottom" | "left" | "right" }) {
  return (
    <Tooltip.Root delayDuration={450}>
      <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          side={side}
          sideOffset={6}
          className="z-[100] flex items-center gap-2 rounded-md bg-raised px-2 py-1 text-xs text-fg shadow-pop animate-fade-in"
        >
          {label}
          {shortcut && <Kbd>{shortcut}</Kbd>}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}

export const IconButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { label: string; shortcut?: string; active?: boolean; side?: "top" | "bottom" | "left" | "right"; dark?: boolean }
>(({ label, shortcut, active, className, side, dark, children, ...props }, ref) => (
  <Tip label={label} shortcut={shortcut} side={side}>
    <button
      ref={ref}
      aria-label={label}
      aria-pressed={active}
      className={cn(
        "inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors disabled:pointer-events-none disabled:opacity-30",
        dark ? "text-vfg/80 hover:bg-vfg/10 hover:text-vfg" : "text-muted hover:bg-raised hover:text-fg",
        active && (dark ? "bg-vfg/15 text-vfg" : "bg-raised text-accent"),
        className,
      )}
      {...props}
    >
      {children}
    </button>
  </Tip>
));
IconButton.displayName = "IconButton";

export const Kbd = ({ children }: { children: ReactNode }) => (
  <kbd className="rounded border border-line bg-panel px-1 font-mono text-[10px] leading-4 text-muted">{children}</kbd>
);

export function Slider({
  value,
  min,
  max,
  step = 0.01,
  onChange,
  onCommit,
  className,
  dark,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  onCommit?: (v: number) => void;
  className?: string;
  dark?: boolean;
}) {
  return (
    <RSlider.Root
      className={cn("relative flex h-5 w-full touch-none select-none items-center", className)}
      value={[value]}
      min={min}
      max={max}
      step={step}
      onValueChange={(v) => onChange(v[0])}
      onValueCommit={(v) => onCommit?.(v[0])}
    >
      <RSlider.Track className={cn("relative h-1 grow overflow-hidden rounded-full", dark ? "bg-vfg/20" : "bg-line")}>
        <RSlider.Range className="absolute h-full bg-accent" />
      </RSlider.Track>
      <RSlider.Thumb className="block h-3.5 w-3.5 rounded-full border-2 border-accent bg-white shadow transition-transform hover:scale-110 focus:outline-none" />
    </RSlider.Root>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label?: string }) {
  return (
    <RSwitch.Root
      checked={checked}
      onCheckedChange={onChange}
      aria-label={label}
      className="relative h-5 w-9 shrink-0 rounded-full bg-line transition-colors data-[state=checked]:bg-accent"
    >
      <RSwitch.Thumb className="block h-4 w-4 translate-x-0.5 rounded-full bg-white shadow transition-transform data-[state=checked]:translate-x-[18px]" />
    </RSwitch.Root>
  );
}

export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: ReactNode; title?: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="inline-flex rounded-md border border-line bg-panel p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          title={o.title}
          onClick={() => onChange(o.value)}
          className={cn(
            "inline-flex h-6 items-center gap-1 rounded px-2 text-xs transition-colors",
            value === o.value ? "bg-raised text-fg shadow-sm" : "text-muted hover:text-fg",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Select<T extends string | number>({ value, options, onChange, className }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; className?: string }) {
  return (
    <select
      value={String(value)}
      onChange={(e) => {
        const o = options.find((x) => String(x.value) === e.target.value);
        if (o) onChange(o.value);
      }}
      className={cn("h-8 rounded-md border border-line bg-panel px-2 text-[13px] text-fg outline-none focus:border-accent", className)}
    >
      {options.map((o) => (
        <option key={String(o.value)} value={String(o.value)}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export const Input = forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn("h-8 w-full rounded-md border border-line bg-panel px-2.5 text-[13px] text-fg outline-none placeholder:text-faint focus:border-accent", className)}
    {...props}
  />
));
Input.displayName = "Input";

export function Spinner({ className }: { className?: string }) {
  return <span className={cn("inline-block h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent", className)} />;
}

export function ProgressBar({ value, total, className }: { value: number; total: number; className?: string }) {
  const pct = total > 0 ? Math.min(100, (value / total) * 100) : 0;
  return (
    <div className={cn("h-1.5 w-full overflow-hidden rounded-full bg-line", className)}>
      <div className="h-full rounded-full bg-accent transition-[width] duration-200" style={{ width: `${pct}%` }} />
    </div>
  );
}
