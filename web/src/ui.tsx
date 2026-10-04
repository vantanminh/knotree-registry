import {
  ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState
} from "react";
import { AlertTriangle, Box, Check, CircleAlert, Copy, Search, X } from "lucide-react";
import { formatAbsolute, formatRelative, splitRepository } from "./format";

export function Logo() {
  return (
    <span className="logo" aria-hidden>
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
        <path d="M4 2.5v11M4 8l5.5-5.5M6.5 6l5.5 7.5" />
      </svg>
    </span>
  );
}

/* ---------- toasts ---------- */

type Toast = { id: number; message: string; tone: "ok" | "error" };
const ToastContext = createContext<(message: string, tone?: Toast["tone"]) => void>(() => undefined);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((message: string, tone: Toast["tone"] = "ok") => {
    const id = Date.now() + Math.random();
    setToasts((current) => [...current.slice(-2), { id, message, tone }]);
    window.setTimeout(() => setToasts((current) => current.filter((item) => item.id !== id)), 2800);
  }, []);
  return (
    <ToastContext.Provider value={push}>
      {children}
      <div className="toast-stack" aria-live="polite">
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast ${toast.tone}`} role="status">
            {toast.tone === "error" ? <CircleAlert size={16} /> : <Check size={16} />}
            {toast.message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  return useContext(ToastContext);
}

/* ---------- layout ---------- */

export function PageHeader({
  title,
  description,
  eyebrow,
  actions,
  badge
}: {
  title: ReactNode;
  description?: ReactNode;
  eyebrow?: ReactNode;
  actions?: ReactNode;
  badge?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div style={{ minWidth: 0 }}>
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1>
          {title}
          {badge}
        </h1>
        {description && <p className="desc">{description}</p>}
      </div>
      {actions && <div className="header-actions">{actions}</div>}
    </header>
  );
}

export function Panel({
  title,
  description,
  actions,
  children,
  className = "",
  bordered = true,
  footer
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
  bordered?: boolean;
  footer?: ReactNode;
}) {
  return (
    <section className={`panel ${className}`}>
      {(title || actions) && (
        <div className={`panel-head ${bordered ? "bordered" : ""}`}>
          <div style={{ minWidth: 0 }}>
            {title && <h2>{title}</h2>}
            {description && <p>{description}</p>}
          </div>
          {actions && <div className="row">{actions}</div>}
        </div>
      )}
      {children}
      {footer && <div className="panel-foot">{footer}</div>}
    </section>
  );
}

export type StatItem = { label: ReactNode; value: ReactNode; unit?: string; detail?: ReactNode; icon?: ReactNode };

export function Stats({ items }: { items: StatItem[] }) {
  return (
    <div className="stats">
      {items.map((item, index) => (
        <div className="stat" key={index}>
          <div className="stat-label">
            {item.icon}
            {item.label}
          </div>
          <div className="stat-value">
            {item.value}
            {item.unit && <small>{item.unit}</small>}
          </div>
          {item.detail && <div className="stat-detail">{item.detail}</div>}
        </div>
      ))}
    </div>
  );
}

/* ---------- copy / code ---------- */

export function useCopy() {
  const toast = useToast();
  return useCallback(
    async (value: string, message = "Copied to clipboard") => {
      try {
        await navigator.clipboard.writeText(value);
        toast(message);
      } catch {
        toast("Clipboard is not available in this browser", "error");
      }
    },
    [toast]
  );
}

export function CopyButton({
  value,
  label,
  className = "btn sm ghost",
  title = "Copy"
}: {
  value: string;
  label?: string;
  className?: string;
  title?: string;
}) {
  const copy = useCopy();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={`${className} ${label ? "" : "icon"}`}
      onClick={async (event) => {
        event.stopPropagation();
        await copy(value);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1400);
      }}
      aria-label={label ?? title}
      title={title}
    >
      {copied ? <Check size={13} /> : <Copy size={13} />}
      {label && (copied ? "Copied" : label)}
    </button>
  );
}

export function CommandBox({ command, label }: { command: string; label?: ReactNode }) {
  return (
    <div style={{ minWidth: 0 }}>
      {label && <div className="cmd-label">{label}</div>}
      <div className="cmd">
        <code>
          <span className="prompt">$</span>
          {command}
        </code>
        <CopyButton value={command} title="Copy command" />
      </div>
    </div>
  );
}

export function DigestView({ value, copy = true }: { value?: string | null; copy?: boolean }) {
  if (!value) return <span className="muted">—</span>;
  const [algo, hash] = value.includes(":") ? value.split(":") : ["", value];
  return (
    <span className="digest" title={value}>
      {algo && <span className="algo">{algo}:</span>}
      {hash.slice(0, 12)}
      {copy && <CopyButton value={value} title="Copy digest" />}
    </span>
  );
}

/* ---------- names / time / status ---------- */

export function RepositoryName({ name, icon = true }: { name: string; icon?: boolean }) {
  const parts = splitRepository(name);
  return (
    <span className="repo-name">
      {icon && (
        <span className="repo-icon" aria-hidden>
          <Box />
        </span>
      )}
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
        <span className="ns">{parts.namespace}/</span>
        <span className="nm">{parts.repository}</span>
      </span>
    </span>
  );
}

export function TagPill({ tag }: { tag?: string | null }) {
  if (!tag) return <span className="muted">—</span>;
  return <span className={tag === "latest" ? "tag-pill latest" : "tag-pill"}>{tag}</span>;
}

export function RelativeTime({ value }: { value?: number | null }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => tick((n) => n + 1), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  if (!value) return <span className="muted">Never</span>;
  return (
    <time dateTime={new Date(value * 1000).toISOString()} title={formatAbsolute(value)}>
      {formatRelative(value)}
    </time>
  );
}

export type Tone = "ok" | "warn" | "fail" | "info" | "accent" | "neutral";

export function StatusBadge({ state, label, dot = false }: { state: Tone; label: ReactNode; dot?: boolean }) {
  return (
    <span className={state === "neutral" ? "badge" : `badge ${state}`}>
      {dot && <i />}
      {label}
    </span>
  );
}

/* ---------- states ---------- */

export function EmptyState({
  title,
  text,
  action,
  icon
}: {
  title: string;
  text: ReactNode;
  action?: ReactNode;
  icon?: ReactNode;
}) {
  return (
    <div className="empty">
      {icon && <div className="empty-icon">{icon}</div>}
      <h3>{title}</h3>
      <p>{text}</p>
      {action && <div className="actions">{action}</div>}
    </div>
  );
}

export function ErrorState({
  message,
  detail,
  onRetry,
  title = "Couldn't load this view"
}: {
  message: string;
  detail?: string;
  onRetry?: () => void;
  title?: string;
}) {
  return (
    <div className="error-state" role="alert">
      <div className="empty-icon">
        <AlertTriangle size={18} />
      </div>
      <h3>{title}</h3>
      <p>{message}</p>
      {onRetry && (
        <div className="actions">
          <button className="btn" onClick={onRetry}>
            Try again
          </button>
        </div>
      )}
      {detail && (
        <details>
          <summary>Details</summary>
          <pre>{detail}</pre>
        </details>
      )}
    </div>
  );
}

export function Skeleton({ rows = 5, stats = false }: { rows?: number; stats?: boolean }) {
  return (
    <div className="skeleton" aria-busy="true" aria-label="Loading">
      <div className="skel title" />
      {stats && <div className="skel block" />}
      {Array.from({ length: rows }, (_, index) => (
        <div className="skel row" key={index} style={{ opacity: 1 - index * 0.12 }} />
      ))}
    </div>
  );
}

/* ---------- inputs ---------- */

export function SearchInput({
  value,
  onChange,
  placeholder = "Search",
  shortcut = true
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  shortcut?: boolean;
}) {
  return (
    <div className="search">
      <Search size={14} />
      <input
        type="search"
        data-page-search
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            onChange("");
            (event.target as HTMLInputElement).blur();
          }
        }}
        placeholder={placeholder}
        aria-label={placeholder}
      />
      {shortcut && !value && <kbd>/</kbd>}
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label
}: {
  value: T;
  options: { id: T; label: ReactNode }[];
  onChange: (id: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          type="button"
          key={option.id}
          role="radio"
          aria-checked={value === option.id}
          className={value === option.id ? "active" : ""}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Tabs({
  value,
  options,
  onChange
}: {
  value: string;
  options: { id: string; label: string; count?: number }[];
  onChange: (id: string) => void;
}) {
  return (
    <div className="tabs" role="tablist">
      {options.map((option) => (
        <button
          key={option.id}
          className={value === option.id ? "tab active" : "tab"}
          role="tab"
          aria-selected={value === option.id}
          onClick={() => onChange(option.id)}
        >
          {option.label}
          {option.count !== undefined && <span className="count">{option.count}</span>}
        </button>
      ))}
    </div>
  );
}

/* ---------- overlays ---------- */

function useEscape(onClose: () => void) {
  const ref = useRef(onClose);
  ref.current = onClose;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") ref.current();
    };
    window.addEventListener("keydown", onKey);
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = previous;
    };
  }, []);
}

export function Modal({
  title,
  description,
  children,
  onClose,
  footer,
  wide
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
}) {
  useEscape(onClose);
  return (
    <div className="overlay center" onMouseDown={onClose} role="presentation">
      <div
        className={`modal ${wide ? "wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="modal-head">
          <div>
            <h2>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <button className="btn icon sm ghost" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

export function Drawer({
  title,
  eyebrow,
  children,
  onClose,
  actions
}: {
  title: ReactNode;
  eyebrow?: ReactNode;
  children: ReactNode;
  onClose: () => void;
  actions?: ReactNode;
}) {
  useEscape(onClose);
  return (
    <div className="overlay" onMouseDown={onClose} role="presentation">
      <aside
        className="drawer"
        role="dialog"
        aria-modal="true"
        aria-label={typeof title === "string" ? title : "Details"}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="drawer-head">
          <div style={{ minWidth: 0 }}>
            {eyebrow && <div className="eyebrow">{eyebrow}</div>}
            <h2>{title}</h2>
          </div>
          <div className="row" style={{ flexWrap: "nowrap" }}>
            {actions}
            <button className="btn icon sm ghost" onClick={onClose} aria-label="Close">
              <X size={16} />
            </button>
          </div>
        </div>
        <div className="drawer-body">{children}</div>
      </aside>
    </div>
  );
}

export function ConfirmDialog({
  title,
  text,
  confirmLabel,
  confirmValue,
  danger,
  onConfirm,
  onClose,
  busy
}: {
  title: string;
  text: ReactNode;
  confirmLabel: string;
  confirmValue?: string;
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
  busy?: boolean;
}) {
  const [typed, setTyped] = useState("");
  const ready = !confirmValue || typed === confirmValue;
  return (
    <Modal
      title={title}
      onClose={onClose}
      footer={
        <>
          <button className="btn ghost" onClick={onClose}>
            Cancel
          </button>
          <button className={`btn ${danger ? "danger solid" : "primary"}`} disabled={!ready || busy} onClick={onConfirm}>
            {busy ? "Working…" : confirmLabel}
          </button>
        </>
      }
    >
      <p style={{ color: "var(--text-2)" }}>{text}</p>
      {confirmValue && (
        <label className="field">
          <span>
            Type <code className="mono">{confirmValue}</code> to confirm
          </span>
          <input
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && ready && !busy) onConfirm();
            }}
            autoFocus
            spellCheck={false}
            autoComplete="off"
          />
        </label>
      )}
    </Modal>
  );
}

export function Callout({
  tone = "info",
  icon,
  children
}: {
  tone?: "info" | "warn" | "error" | "ok";
  icon?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={`callout ${tone}`} role={tone === "error" ? "alert" : undefined}>
      {icon ?? <AlertTriangle size={15} />}
      <div>{children}</div>
    </div>
  );
}

/* ---------- charts ---------- */

export function Meter({ parts }: { parts: { value: number; color: string; label: string }[] }) {
  const total = parts.reduce((sum, part) => sum + part.value, 0);
  return (
    <div className="meter" role="img" aria-label={parts.map((p) => `${p.label} ${p.value}`).join(", ")}>
      {total > 0 &&
        parts
          .filter((part) => part.value > 0)
          .map((part) => (
            <span key={part.label} style={{ width: `${(part.value / total) * 100}%`, background: part.color }} />
          ))}
    </div>
  );
}

/* ---------- prefs ---------- */

export type ThemePref = "system" | "light" | "dark";

export function useTheme() {
  const [theme, setTheme] = useState<ThemePref>(() => {
    try {
      return (localStorage.getItem("kntr-theme") as ThemePref) ?? "system";
    } catch {
      return "system";
    }
  });
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const resolved = theme === "system" ? (media.matches ? "light" : "dark") : theme;
      document.documentElement.dataset.theme = resolved;
    };
    apply();
    try {
      localStorage.setItem("kntr-theme", theme);
    } catch {
      /* storage unavailable */
    }
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
  return { theme, setTheme };
}

export function useCollapsed() {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem("kntr-sidebar") === "1";
    } catch {
      return false;
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem("kntr-sidebar", collapsed ? "1" : "0");
    } catch {
      /* storage unavailable */
    }
  }, [collapsed]);
  return { collapsed, setCollapsed };
}

export function permissionHelp(action: string): string {
  if (action === "pull") return "Download images and read manifests.";
  if (action === "push") return "Upload layers and publish tags.";
  if (action === "delete") return "Delete manifests and tags.";
  return "Manage repository access and settings.";
}
