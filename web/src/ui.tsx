import {
  ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState
} from "react";
import { Check, Copy, Search, X } from "lucide-react";
import { formatAbsolute, formatRelative, splitRepository, truncateDigest } from "./format";

type Toast = { id: number; message: string; tone?: "ok" | "error" };
const ToastContext = createContext<(message: string, tone?: Toast["tone"]) => void>(() => undefined);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((message: string, tone: Toast["tone"] = "ok") => {
    const id = Date.now() + Math.random();
    setToasts((current) => [...current.slice(-3), { id, message, tone }]);
    window.setTimeout(() => setToasts((current) => current.filter((item) => item.id !== id)), 2400);
  }, []);
  return (
    <ToastContext.Provider value={push}>
      {children}
      <div className="toast-stack" aria-live="polite">
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast ${toast.tone === "error" ? "alert error" : ""}`}>
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

export function PageHeader({
  title,
  description,
  actions
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="page-header">
      <div>
        <h1>{title}</h1>
        {description && <p>{description}</p>}
      </div>
      {actions && <div className="header-actions">{actions}</div>}
    </div>
  );
}

export function Metric({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
      {detail && <small>{detail}</small>}
    </div>
  );
}

export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  const toast = useToast();
  const [copied, setCopied] = useState(false);
  async function copy() {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    toast("Copied to clipboard");
    window.setTimeout(() => setCopied(false), 1400);
  }
  return (
    <button type="button" className="btn sm ghost" onClick={copy} aria-label={label}>
      {copied ? <Check size={14} /> : <Copy size={14} />}
      {copied ? "Copied" : label}
    </button>
  );
}

export function CommandBox({ command }: { command: string }) {
  return (
    <div className="command-box">
      <code>{command}</code>
      <CopyButton value={command} />
    </div>
  );
}

export function DigestView({ value }: { value?: string | null }) {
  if (!value) return <span className="muted">—</span>;
  return (
    <span className="digest" title={value}>
      {truncateDigest(value)}
      <CopyButton value={value} label="" />
    </span>
  );
}

export function RepositoryName({ name }: { name: string }) {
  const parts = splitRepository(name);
  return (
    <span>
      <span className="repo-ns">{parts.namespace} / </span>
      <span className="repo-primary">{parts.repository}</span>
    </span>
  );
}

export function RelativeTime({ value }: { value?: number | null }) {
  return <span title={formatAbsolute(value)}>{formatRelative(value)}</span>;
}

export function StatusBadge({
  state,
  label
}: {
  state: "ok" | "warn" | "fail" | "info" | "neutral";
  label: string;
}) {
  const className = state === "neutral" ? "badge" : `badge ${state}`;
  return <span className={className}>{label}</span>;
}

export function EmptyState({
  title,
  text,
  action
}: {
  title: string;
  text: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <h3>{title}</h3>
      <p>{text}</p>
      {action}
    </div>
  );
}

export function ErrorState({
  message,
  detail,
  onRetry
}: {
  message: string;
  detail?: string;
  onRetry?: () => void;
}) {
  return (
    <div className="error-state">
      <h3>Could not load this view</h3>
      <p>{message}</p>
      {onRetry && (
        <button className="btn" onClick={onRetry}>
          Retry
        </button>
      )}
      {detail && (
        <details className="details">
          <summary>Show details</summary>
          <pre>{detail}</pre>
        </details>
      )}
    </div>
  );
}

export function Skeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div className="skeleton">
      {Array.from({ length: rows }, (_, index) => (
        <div className={`skel ${index === 0 ? "lg" : ""}`} key={index} />
      ))}
    </div>
  );
}

export function SearchInput({
  value,
  onChange,
  placeholder = "Search..."
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  return (
    <div className="search">
      <Search size={14} />
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        aria-label={placeholder}
      />
    </div>
  );
}

export function Modal({
  title,
  children,
  onClose,
  footer,
  wide
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
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
          <h2>{title}</h2>
          <button className="btn icon ghost" onClick={onClose} aria-label="Close">
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
  children,
  onClose
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={onClose} role="presentation">
      <aside className="drawer" role="dialog" aria-modal="true" aria-label={title} onMouseDown={(event) => event.stopPropagation()}>
        <div className="drawer-head">
          <h2>{title}</h2>
          <button className="btn icon ghost" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
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
  text: string;
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
      <p>{text}</p>
      {confirmValue && (
        <label className="field">
          <span>Type {confirmValue} to confirm</span>
          <input value={typed} onChange={(event) => setTyped(event.target.value)} autoFocus />
        </label>
      )}
    </Modal>
  );
}

export function HealthDot({ status }: { status: string }) {
  const ok = status === "ok" || status === "healthy" || status === "operational" || status === "skipped";
  const fail = status === "failed" || status === "not_ready" || status === "missing";
  return <i className={`dot ${ok ? "ok" : fail ? "fail" : "warn"}`} aria-hidden />;
}

export function useTheme() {
  const [theme, setTheme] = useState(() => localStorage.getItem("kntr-theme") ?? "system");
  useEffect(() => {
    const resolved =
      theme === "system"
        ? window.matchMedia("(prefers-color-scheme: light)").matches
          ? "light"
          : "dark"
        : theme;
    document.documentElement.dataset.theme = resolved;
    localStorage.setItem("kntr-theme", theme);
  }, [theme]);
  return { theme, setTheme };
}

export function useCollapsed() {
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem("kntr-sidebar") === "1");
  useEffect(() => {
    localStorage.setItem("kntr-sidebar", collapsed ? "1" : "0");
  }, [collapsed]);
  return { collapsed, setCollapsed };
}

export function useDebounced<T>(value: T, delay = 250): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), delay);
    return () => window.clearTimeout(timer);
  }, [value, delay]);
  return debounced;
}

export function permissionHelp(action: string): string {
  if (action === "pull") return "Allows downloading images.";
  if (action === "push") return "Allows uploading images.";
  if (action === "delete") return "Allows deleting manifests.";
  return "Allows managing repository permissions and settings.";
}

export function Tabs({
  value,
  options,
  onChange
}: {
  value: string;
  options: { id: string; label: string }[];
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
        </button>
      ))}
    </div>
  );
}
