import * as CM from "@radix-ui/react-context-menu";
import * as DM from "@radix-ui/react-dropdown-menu";
import { Check, ChevronRight } from "lucide-react";
import { useState, type ReactNode } from "react";
import { cn } from "../../lib/format";
import { Kbd } from "./primitives";

const content = "z-[90] min-w-[200px] overflow-hidden rounded-lg border border-line bg-panel p-1 text-[13px] text-fg shadow-pop animate-scale-in";
const item =
  "relative flex h-8 cursor-default select-none items-center gap-2 rounded-md px-2 outline-none data-[disabled]:pointer-events-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-fg data-[disabled]:opacity-40";

export interface MenuEntry {
  label?: string;
  icon?: ReactNode;
  shortcut?: string;
  onSelect?: () => void;
  disabled?: boolean;
  danger?: boolean;
  checked?: boolean;
  separator?: boolean;
  heading?: string;
  submenu?: MenuEntry[];
  hidden?: boolean;
}

type Kit = typeof DM | typeof CM;

function renderEntries(kit: Kit, entries: MenuEntry[]): ReactNode {
  return entries
    .filter((e) => !e.hidden)
    .map((e, i) => {
      if (e.separator) return <kit.Separator key={i} className="my-1 h-px bg-line" />;
      if (e.heading) return <kit.Label key={i} className="px-2 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wide text-faint">{e.heading}</kit.Label>;
      if (e.submenu)
        return (
          <kit.Sub key={i}>
            <kit.SubTrigger className={cn(item, "data-[state=open]:bg-raised")} disabled={e.disabled}>
              <span className="flex w-4 justify-center">{e.icon}</span>
              <span className="flex-1">{e.label}</span>
              <ChevronRight className="h-3.5 w-3.5 opacity-60" />
            </kit.SubTrigger>
            <kit.Portal>
              <kit.SubContent className={cn(content, "max-h-[60vh] overflow-y-auto")} sideOffset={4}>
                {renderEntries(kit, e.submenu)}
              </kit.SubContent>
            </kit.Portal>
          </kit.Sub>
        );
      return (
        <kit.Item key={i} className={cn(item, e.danger && "text-danger data-[highlighted]:bg-danger data-[highlighted]:text-white")} disabled={e.disabled} onSelect={e.onSelect}>
          <span className="flex w-4 justify-center">{e.checked ? <Check className="h-3.5 w-3.5" /> : e.icon}</span>
          <span className="flex-1 truncate">{e.label}</span>
          {e.shortcut && <Kbd>{e.shortcut}</Kbd>}
        </kit.Item>
      );
    });
}

export function DropdownMenu({ trigger, entries, align = "end" }: { trigger: ReactNode; entries: MenuEntry[]; align?: "start" | "end" | "center" }) {
  return (
    <DM.Root modal={false}>
      <DM.Trigger asChild>{trigger}</DM.Trigger>
      <DM.Portal>
        <DM.Content className={content} align={align} sideOffset={6} onCloseAutoFocus={(e) => e.preventDefault()}>
          {renderEntries(DM, entries)}
        </DM.Content>
      </DM.Portal>
    </DM.Root>
  );
}

/** Dropdown with arbitrary content (filter panel etc.). */
export function Popover({ trigger, children, align = "end" }: { trigger: ReactNode; children: ReactNode; align?: "start" | "end" }) {
  return (
    <DM.Root modal={false}>
      <DM.Trigger asChild>{trigger}</DM.Trigger>
      <DM.Portal>
        <DM.Content className={cn(content, "p-3")} align={align} sideOffset={6} onCloseAutoFocus={(e) => e.preventDefault()} onKeyDown={(e) => e.stopPropagation()}>
          {children}
        </DM.Content>
      </DM.Portal>
    </DM.Root>
  );
}

/**
 * Context menu whose entries are built lazily when it opens (never during
 * render), so `entries`/`onOpen` may read or update stores freely.
 */
export function ContextMenu({ children, entries, onOpen }: { children: ReactNode; entries: () => MenuEntry[]; onOpen?: () => void }) {
  const [items, setItems] = useState<MenuEntry[] | null>(null);
  return (
    <CM.Root
      modal={false}
      onOpenChange={(open) => {
        if (open) {
          onOpen?.();
          setItems(entries());
        } else setItems(null);
      }}
    >
      <CM.Trigger asChild>{children}</CM.Trigger>
      {items && (
        <CM.Portal>
          <CM.Content className={content}>{renderEntries(CM, items)}</CM.Content>
        </CM.Portal>
      )}
    </CM.Root>
  );
}
