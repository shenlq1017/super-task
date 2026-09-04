import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** Editable model picker. Unlike native datalist this renders consistently in
 * Tauri WebView and keeps the options in a floating layer without resizing the form. */
export function ModelCombobox(props: {
  value: string;
  options: string[];
  onChange: (value: string) => void;
  placeholder?: string;
  emptyText: string;
  disabled?: boolean;
  ariaLabel: string;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [filtering, setFiltering] = useState(false);

  const options = useMemo(
    () => [...new Set(props.options.map((item) => item.trim()).filter(Boolean))],
    [props.options],
  );
  const query = filtering ? props.value.trim().toLocaleLowerCase() : "";
  const filtered = useMemo(
    () => options.filter((item) => !query || item.toLocaleLowerCase().includes(query)),
    [options, query],
  );

  useEffect(() => {
    // Every successful fetch reveals the fresh result immediately; no second click.
    if (options.length > 0) {
      setFiltering(false);
      setOpen(true);
      setActive(0);
    }
  }, [options]);

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [open]);

  const choose = (model: string) => {
    props.onChange(model);
    setFiltering(false);
    setOpen(false);
  };

  return (
    <div ref={root} className="relative min-w-0 flex-1">
      <Input
        value={props.value}
        onChange={(event) => {
          setFiltering(true);
          props.onChange(event.target.value);
          if (options.length) setOpen(true);
        }}
        onFocus={() => { if (options.length) { setFiltering(false); setOpen(true); } }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            setActive((index) => Math.min(index + 1, Math.max(0, filtered.length - 1)));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActive((index) => Math.max(0, index - 1));
          } else if (event.key === "Enter" && open && filtered[active]) {
            event.preventDefault();
            choose(filtered[active]);
          } else if (event.key === "Escape") {
            setOpen(false);
          }
        }}
        className="w-full pr-9 font-mono"
        placeholder={props.placeholder}
        disabled={props.disabled}
        role="combobox"
        aria-label={props.ariaLabel}
        aria-expanded={open}
        aria-autocomplete="list"
        aria-controls="ai-model-options"
        autoComplete="off"
        spellCheck={false}
      />
      <Button
        variant="ghost"
        size="icon-sm"
        type="button"
        className="absolute right-1 top-1/2 -translate-y-1/2 text-[var(--t3)]"
        onClick={() => { if (options.length) { setFiltering(false); setOpen((value) => !value); } }}
        disabled={props.disabled || options.length === 0}
        aria-label={props.ariaLabel}
        tabIndex={-1}
      >
        <ChevronDown className={cn("size-3.5 transition-transform", open && "rotate-180")} />
      </Button>
      {open && options.length ? (
        <div
          id="ai-model-options"
          role="listbox"
          className="absolute left-0 right-0 top-full z-[120] mt-1 max-h-52 overflow-y-auto rounded-[var(--r-sm,8px)] border border-[var(--line-strong)] bg-[var(--surface)] p-1 shadow-[0_10px_28px_rgb(0_0_0/0.16)]"
        >
          {filtered.length ? (
            filtered.map((model, index) => (
              <button
                key={model}
                type="button"
                role="option"
                aria-selected={model === props.value}
                onMouseEnter={() => setActive(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => choose(model)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-[6px] px-2 py-1.5 text-left font-mono text-[0.75rem] transition-colors",
                  index === active ? "bg-[var(--surface-2)] text-[var(--t1)]" : "text-[var(--t2)]",
                )}
              >
                <Check className={cn("size-3.5 shrink-0", model === props.value ? "opacity-100" : "opacity-0")} />
                <span className="min-w-0 flex-1 break-all">{model}</span>
              </button>
            ))
          ) : (
            <p className="px-2 py-2 text-[0.72rem] text-[var(--t3)]">{props.emptyText}</p>
          )}
        </div>
      ) : null}
    </div>
  );
}
