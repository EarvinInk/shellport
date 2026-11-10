import clsx from "clsx";
import { X } from "lucide-react";
import { useEffect, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";

type Variant = "primary" | "ghost" | "danger" | "subtle";

export function Button({
  variant = "subtle",
  size = "md",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }) {
  return (
    <button
      {...props}
      className={clsx(
        "inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium transition-colors disabled:opacity-40 disabled:pointer-events-none cursor-pointer select-none",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-9 px-3.5 text-sm",
        variant === "primary" && "bg-accent text-white hover:bg-accent-strong",
        variant === "subtle" && "bg-surface-3 text-fg hover:bg-surface-4",
        variant === "ghost" && "text-fg-muted hover:text-fg hover:bg-surface-3",
        variant === "danger" && "bg-danger/15 text-danger hover:bg-danger/25",
        className,
      )}
    />
  );
}

export function IconButton({ title, className, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { title: string }) {
  return (
    <button
      title={title}
      aria-label={title}
      {...props}
      className={clsx(
        "inline-flex h-7 w-7 items-center justify-center rounded-md text-fg-muted hover:bg-surface-3 hover:text-fg disabled:opacity-40 cursor-pointer",
        className,
      )}
    />
  );
}

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={clsx(
        "h-9 w-full rounded-md border border-border bg-surface-1 px-3 text-sm text-fg placeholder:text-fg-subtle outline-none focus:border-accent",
        className,
      )}
    />
  );
}

export function TextArea({ className, ...props }: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={clsx(
        "w-full rounded-md border border-border bg-surface-1 px-3 py-2.5 text-sm text-fg font-mono placeholder:text-fg-subtle outline-none focus:border-accent",
        className,
      )}
    />
  );
}

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={clsx(
        "h-9 w-full rounded-md border border-border bg-surface-1 px-3 text-sm text-fg outline-none focus:border-accent",
        className,
      )}
    >
      {children}
    </select>
  );
}

export function Field({ label, hint, children, className }: { label: string; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={clsx("flex flex-col gap-1.5", className)}>
      <span className="text-xs font-medium text-fg-muted">{label}</span>
      {children}
      {hint && <span className="text-xs text-fg-subtle">{hint}</span>}
    </label>
  );
}

export function Checkbox({ label, checked, onChange, disabled }: { label: string; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <label className={clsx("flex items-center gap-2 text-sm text-fg select-none", disabled ? "opacity-60" : "cursor-pointer")}>
      <input type="checkbox" disabled={disabled} className="accent-[var(--color-accent)] h-4 w-4" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
}

export function Modal({
  title,
  onClose,
  children,
  footer,
  width = "max-w-xl",
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
}) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4" onMouseDown={onClose}>
      <div
        className={clsx("flex max-h-[88vh] w-full flex-col rounded-xl border border-border bg-surface-2 shadow-2xl", width)}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-base font-semibold">{title}</h2>
          <IconButton title="Close" onClick={onClose}>
            <X size={16} />
          </IconButton>
        </div>
        <div className="overflow-y-auto px-6 py-5">{children}</div>
        {footer && <div className="flex items-center justify-end gap-2 border-t border-border px-6 py-4">{footer}</div>}
      </div>
    </div>
  );
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "ok" | "warn" | "bad" | "info" }) {
  return (
    <span
      className={clsx(
        "inline-flex items-center rounded px-1.5 py-0.5 text-[11px] font-medium",
        tone === "neutral" && "bg-surface-3 text-fg-muted",
        tone === "ok" && "bg-ok/15 text-ok",
        tone === "warn" && "bg-warn/15 text-warn",
        tone === "bad" && "bg-danger/15 text-danger",
        tone === "info" && "bg-accent/15 text-accent",
      )}
    >
      {children}
    </span>
  );
}

export function Empty({ icon, title, children }: { icon?: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      {icon && <div className="text-fg-subtle">{icon}</div>}
      <div className="text-sm font-medium">{title}</div>
      {children && <div className="max-w-md text-sm text-fg-muted">{children}</div>}
    </div>
  );
}

/** The Production marker as a small badge (sidebars, tabs, palette). */
export function ProdBadge({ className }: { className?: string }) {
  return (
    <span className={clsx("shrink-0 rounded bg-danger/15 px-1 py-px text-[9px] font-bold uppercase tracking-wider text-danger", className)} title="Production: risky actions ask first">
      Prod
    </span>
  );
}
