import { ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Link, Navigate, Outlet, Route, Routes, useLocation, useNavigate, useSearchParams } from "react-router-dom";
import {
  Activity,
  ArrowRight,
  BookOpen,
  Box,
  Boxes,
  ChevronsUpDown,
  CircleDot,
  Cloud,
  HardDrive,
  KeyRound,
  LayoutGrid,
  LogOut,
  Menu,
  Monitor,
  Moon,
  PanelLeft,
  Plus,
  ScrollText,
  Search,
  Settings,
  ShieldCheck,
  Sun,
  Terminal,
  Trash2,
  Upload,
  Users,
  Webhook
} from "lucide-react";
import { Instance, User, api } from "./api";
import { CloudConnectionPage } from "./pages-cloud";
import { ActivityPage, LoginPage, OverviewPage, StatusPage } from "./pages-overview";
import { NamespacesPage, RepositoriesPage, RepositoryDetailPage } from "./pages-repos";
import { TokenDetailPage, TokensPage } from "./pages-tokens";
import {
  AuditPage,
  GarbageCollectionPage,
  MembersPage,
  SecuritySettingsPage,
  SettingsPage,
  StoragePage,
  UploadsPage,
  WebhooksPage
} from "./pages-ops";
import { Session, SessionContext } from "./session";
import { Logo, Segmented, ThemePref, ToastProvider, useCollapsed, useCopy, useTheme } from "./ui";

const modKey = /Mac|iPhone|iPad/.test(navigator.userAgent) ? "⌘" : "Ctrl";

type NavItem = { to: string; label: string; icon: typeof Box; admin?: boolean; match?: (path: string) => boolean };

const nav: { label: string; items: NavItem[] }[] = [
  {
    label: "Registry",
    items: [
      { to: "/", label: "Overview", icon: LayoutGrid, match: (path) => path === "/" },
      { to: "/repositories", label: "Repositories", icon: Box },
      { to: "/namespaces", label: "Namespaces", icon: Boxes, match: (path) => path === "/namespaces" },
      { to: "/activity", label: "Activity", icon: Activity }
    ]
  },
  {
    label: "Delivery",
    items: [
      { to: "/deployments/cloud", label: "Knotree Cloud", icon: Cloud },
      { to: "/operations/webhooks", label: "Webhooks", icon: Webhook, admin: true }
    ]
  },
  {
    label: "Access",
    items: [
      { to: "/security/tokens", label: "Access tokens", icon: KeyRound },
      { to: "/namespaces/members", label: "Members", icon: Users },
      { to: "/security/audit", label: "Audit log", icon: ScrollText }
    ]
  },
  {
    label: "Operations",
    items: [
      { to: "/operations/storage", label: "Storage", icon: HardDrive },
      { to: "/operations/garbage-collection", label: "Garbage collection", icon: Trash2, admin: true },
      { to: "/uploads", label: "Uploads", icon: Upload, admin: true },
      { to: "/settings/general", label: "Settings", icon: Settings, match: (path) => path.startsWith("/settings/") && path !== "/settings/security" }
    ]
  }
];

const titles: Record<string, string> = {
  repositories: "Repositories",
  namespaces: "Namespaces",
  members: "Members",
  uploads: "Uploads",
  deployments: "Delivery",
  cloud: "Knotree Cloud",
  activity: "Activity",
  operations: "Operations",
  storage: "Storage",
  webhooks: "Webhooks",
  "garbage-collection": "Garbage collection",
  security: "Access",
  tokens: "Access tokens",
  audit: "Audit log",
  settings: "Settings",
  general: "General",
  registry: "Registry",
  retention: "Retention",
  account: "Account",
  status: "System status"
};

export default function App() {
  return (
    <ToastProvider>
      <Routes>
        <Route path="/login" element={<AuthGate />} />
        <Route element={<Shell />}>
          <Route path="/" element={<OverviewPage />} />
          <Route path="/repositories" element={<RepositoriesPage />} />
          <Route path="/repositories/*" element={<RepositoryDetailPage />} />
          <Route path="/namespaces" element={<NamespacesPage />} />
          <Route path="/namespaces/members" element={<MembersPage />} />
          <Route path="/uploads" element={<UploadsPage />} />
          <Route path="/deployments/cloud" element={<CloudConnectionPage />} />
          <Route path="/deployments/watchers" element={<Navigate to="/deployments/cloud" replace />} />
          <Route path="/deployments/agents" element={<Navigate to="/deployments/cloud" replace />} />
          <Route path="/activity" element={<ActivityPage />} />
          <Route path="/operations/storage" element={<StoragePage />} />
          <Route path="/operations/webhooks" element={<WebhooksPage />} />
          <Route path="/operations/garbage-collection" element={<GarbageCollectionPage />} />
          <Route path="/security/tokens" element={<TokensPage />} />
          <Route path="/security/tokens/:id" element={<TokenDetailPage />} />
          <Route path="/security/audit" element={<AuditPage />} />
          <Route path="/settings/general" element={<SettingsPage />} />
          <Route path="/settings/registry" element={<SettingsPage />} />
          <Route path="/settings/retention" element={<SettingsPage />} />
          <Route path="/settings/security" element={<SecuritySettingsPage />} />
          <Route path="/account/security" element={<SecuritySettingsPage />} />
          <Route path="/status" element={<StatusPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </ToastProvider>
  );
}

function safeReturnTo(value: string | null) {
  return value?.startsWith("/") && !value.startsWith("//") && !value.includes("\\") && !/[\u0000-\u001f\u007f]/.test(value) ? value : "/";
}

function Splash({ label }: { label: string }) {
  return (
    <div className="splash">
      <div className="splash-inner">
        <Logo />
        <span>{label}</span>
      </div>
    </div>
  );
}

function AuthGate() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const [params] = useSearchParams();
  useEffect(() => {
    api<User>("/api/v1/auth/me")
      .then(setUser)
      .catch(() => setUser(null));
  }, []);
  if (user === undefined) return <Splash label="Checking your session…" />;
  if (user) return <Navigate to={safeReturnTo(params.get("returnTo"))} replace />;
  return <LoginPage returnTo={safeReturnTo(params.get("returnTo"))} />;
}

/** Loads the session once, then renders the dashboard chrome around the routed page. */
function Shell() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const [instance, setInstance] = useState<Instance | null>(null);
  const location = useLocation();
  useEffect(() => {
    api<User>("/api/v1/auth/me")
      .then(setUser)
      .catch(() => setUser(null));
    api<Instance>("/api/v1/instance")
      .then(setInstance)
      .catch(() => setInstance(null));
  }, []);
  const session = useMemo<Session | null>(
    () => (user ? { user, instance, host: instance?.registry_host ?? "registry.knotree.com" } : null),
    [user, instance]
  );
  if (user === undefined) return <Splash label="Loading workspace…" />;
  if (!session) return <Navigate to={`/login?returnTo=${encodeURIComponent(location.pathname + location.search)}`} replace />;
  return (
    <SessionContext.Provider value={session}>
      <Chrome session={session} />
    </SessionContext.Provider>
  );
}

function Chrome({ session }: { session: Session }) {
  const { user, instance, host } = session;
  const [menuOpen, setMenuOpen] = useState(false);
  const [palette, setPalette] = useState(false);
  const { collapsed, setCollapsed } = useCollapsed();
  const { theme, setTheme } = useTheme();
  const location = useLocation();
  const navigate = useNavigate();

  useEffect(() => setMenuOpen(false), [location.pathname]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      const typing = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT" || target.isContentEditable;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPalette((open) => !open);
      } else if (event.key === "/" && !typing) {
        const search = document.querySelector<HTMLInputElement>("input[data-page-search]");
        event.preventDefault();
        if (search) search.focus();
        else setPalette(true);
      } else if ((event.metaKey || event.ctrlKey) && event.key === "\\") {
        event.preventDefault();
        setCollapsed(!collapsed);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [collapsed, setCollapsed]);

  const visible = nav
    .map((group) => ({ ...group, items: group.items.filter((item) => !item.admin || user.is_admin) }))
    .filter((group) => group.items.length);

  async function logout() {
    await api("/api/v1/auth/logout", { method: "POST" }).catch(() => undefined);
    navigate("/login");
  }

  return (
    <div className="app-shell">
      <aside className={`sidebar ${collapsed ? "collapsed" : ""} ${menuOpen ? "open" : ""}`} aria-label="Primary">
        <div className="sidebar-top">
          <Link to="/" className="instance" title={host}>
            <Logo />
            <span className="instance-meta hide-collapsed">
              <strong>Knotree Registry</strong>
              <span>{host}</span>
            </span>
          </Link>
          <button className="search-trigger" onClick={() => setPalette(true)} aria-label="Search and commands">
            <Search size={14} />
            <span className="hide-collapsed">Search…</span>
            <kbd className="hide-collapsed">{modKey} K</kbd>
          </button>
        </div>
        <nav className="nav-scroll">
          {visible.map((group) => (
            <div className="nav-group" key={group.label}>
              <div className="nav-group-label">{group.label}</div>
              {group.items.map((item) => {
                const active = item.match ? item.match(location.pathname) : location.pathname.startsWith(item.to);
                return (
                  <Link
                    key={item.to}
                    className={active ? "nav-item active" : "nav-item"}
                    to={item.to}
                    title={collapsed ? item.label : undefined}
                    aria-current={active ? "page" : undefined}
                  >
                    <item.icon />
                    <span className="hide-collapsed">{item.label}</span>
                  </Link>
                );
              })}
            </div>
          ))}
        </nav>
        <div className="sidebar-foot">
          <Link className={location.pathname === "/status" ? "nav-item active" : "nav-item"} to="/status" title={collapsed ? "System status" : undefined}>
            <CircleDot />
            <span className="hide-collapsed">System status</span>
          </Link>
          <UserMenu user={user} theme={theme} setTheme={setTheme} onLogout={logout} />
        </div>
      </aside>
      <div className={`scrim ${menuOpen ? "show" : ""}`} onClick={() => setMenuOpen(false)} />
      <div className="main">
        <header className="topbar">
          <button className="btn icon ghost mobile-only" onClick={() => setMenuOpen(true)} aria-label="Open navigation">
            <Menu size={17} />
          </button>
          <button
            className="btn icon ghost desktop-only"
            onClick={() => setCollapsed(!collapsed)}
            aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
            title={`${collapsed ? "Expand" : "Collapse"} sidebar (Ctrl \\)`}
          >
            <PanelLeft size={16} />
          </button>
          <Breadcrumbs path={location.pathname} />
          <div className="top-actions">
            <span className="env-pill desktop-only" title="Environment">
              <i className="dot ok" style={{ width: 6, height: 6, flexBasis: 6, boxShadow: "none" }} />
              {instance?.environment ?? "registry"}
            </span>
            <button className="btn icon ghost mobile-only" onClick={() => setPalette(true)} aria-label="Search">
              <Search size={16} />
            </button>
            {!location.pathname.startsWith("/security/tokens") && (
              <Link className="btn sm primary" to="/security/tokens?create=1">
                <Plus size={14} />
                <span className="desktop-only">New token</span>
              </Link>
            )}
          </div>
        </header>
        <main className="content" key={location.pathname}>
          <Outlet />
        </main>
      </div>
      {palette && (
        <CommandPalette
          admin={user.is_admin}
          host={host}
          setTheme={setTheme}
          onClose={() => setPalette(false)}
          onNavigate={(path) => {
            setPalette(false);
            navigate(path);
          }}
        />
      )}
    </div>
  );
}

function UserMenu({
  user,
  theme,
  setTheme,
  onLogout
}: {
  user: User;
  theme: ThemePref;
  setTheme: (theme: ThemePref) => void;
  onLogout: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    const esc = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);
  return (
    <div ref={ref}>
      <button className="user-button" onClick={() => setOpen(!open)} aria-expanded={open} aria-haspopup="menu">
        <span className="avatar">{user.username.slice(0, 2).toUpperCase()}</span>
        <span className="user-meta hide-collapsed">
          <strong>{user.username}</strong>
          <span>{user.is_admin ? "Administrator" : "Member"}</span>
        </span>
        <ChevronsUpDown size={14} className="chev hide-collapsed" />
      </button>
      {open && (
        <div className="menu up" role="menu">
          <div className="menu-label">Theme</div>
          <Segmented
            label="Theme"
            value={theme}
            onChange={setTheme}
            options={[
              { id: "system", label: <Monitor size={14} aria-label="System" /> },
              { id: "light", label: <Sun size={14} aria-label="Light" /> },
              { id: "dark", label: <Moon size={14} aria-label="Dark" /> }
            ]}
          />
          <div className="menu-sep" />
          <Link className="menu-item" to="/account/security" role="menuitem" onClick={() => setOpen(false)}>
            <ShieldCheck /> Account & security
          </Link>
          <a className="menu-item" href="https://github.com/knotree/registry" target="_blank" rel="noreferrer" role="menuitem">
            <BookOpen /> Documentation
          </a>
          <div className="menu-sep" />
          <button className="menu-item" onClick={onLogout} role="menuitem">
            <LogOut /> Sign out
          </button>
        </div>
      )}
    </div>
  );
}

function Breadcrumbs({ path }: { path: string }) {
  if (path === "/") {
    return (
      <nav className="crumbs" aria-label="Breadcrumb">
        <span className="current">Overview</span>
      </nav>
    );
  }
  const parts = path.split("/").filter(Boolean);
  const crumbs: { to: string; label: string }[] = [];
  let acc = "";
  const repoDetail = parts[0] === "repositories" && parts.length > 1;
  if (path === "/account/security" || path === "/settings/security") {
    crumbs.push({ to: path, label: "Account & security" });
  } else if (repoDetail) {
    crumbs.push({ to: "/repositories", label: "Repositories" });
    crumbs.push({ to: path, label: decodeURIComponent(parts.slice(1).join("/")) });
  } else {
    for (const part of parts) {
      acc += `/${part}`;
      crumbs.push({ to: acc, label: titles[part] ?? decodeURIComponent(part) });
    }
  }
  return (
    <nav className="crumbs" aria-label="Breadcrumb">
      {crumbs.map((crumb, index) => {
        const last = index === crumbs.length - 1;
        const linkable = !["/deployments", "/operations", "/security", "/settings", "/account"].includes(crumb.to);
        return (
          <span key={crumb.to} style={{ display: "contents" }}>
            {index > 0 && <span className="sep">/</span>}
            {last ? (
              <span className="current">{crumb.label}</span>
            ) : linkable ? (
              <Link to={crumb.to}>{crumb.label}</Link>
            ) : (
              <span>{crumb.label}</span>
            )}
          </span>
        );
      })}
    </nav>
  );
}

type PaletteItem = { group: string; label: string; icon: ReactNode; run: () => void; hint?: string; keywords?: string };

function CommandPalette({
  admin,
  host,
  setTheme,
  onClose,
  onNavigate
}: {
  admin: boolean;
  host: string;
  setTheme: (theme: ThemePref) => void;
  onClose: () => void;
  onNavigate: (path: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [repos, setRepos] = useState<string[]>([]);
  const copy = useCopy();
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    api<{ repositories: { name: string }[] }>("/api/v1/repositories")
      .then((result) => setRepos(result.repositories.map((repo) => repo.name)))
      .catch(() => setRepos([]));
  }, []);

  const pages: PaletteItem[] = nav
    .flatMap((group) => group.items)
    .filter((item) => !item.admin || admin)
    .map((item) => ({ group: "Go to", label: item.label, icon: <item.icon />, run: () => onNavigate(item.to), hint: "Page" }));
  const actions: PaletteItem[] = [
    { group: "Actions", label: "Create access token", icon: <Plus />, run: () => onNavigate("/security/tokens?create=1") },
    {
      group: "Actions",
      label: "Copy docker login command",
      icon: <Terminal />,
      run: () => {
        copy(`docker login ${host}`);
        onClose();
      },
      hint: "Copy"
    },
    { group: "Actions", label: "View system status", icon: <CircleDot />, run: () => onNavigate("/status") },
    { group: "Actions", label: "Switch to light theme", icon: <Sun />, run: () => { setTheme("light"); onClose(); }, keywords: "appearance" },
    { group: "Actions", label: "Switch to dark theme", icon: <Moon />, run: () => { setTheme("dark"); onClose(); }, keywords: "appearance" },
    { group: "Actions", label: "Use system theme", icon: <Monitor />, run: () => { setTheme("system"); onClose(); }, keywords: "appearance" }
  ];
  const q = query.trim().toLowerCase();
  const repoItems: PaletteItem[] = repos
    .filter((name) => !q || name.includes(q))
    .slice(0, q ? 8 : 5)
    .map((name) => ({ group: "Repositories", label: name, icon: <Box />, run: () => onNavigate(`/repositories/${name}`), hint: "Repository" }));
  const match = (item: PaletteItem) => !q || `${item.label} ${item.keywords ?? ""}`.toLowerCase().includes(q);
  const items = [...repoItems, ...pages.filter(match), ...actions.filter(match)];

  useEffect(() => {
    listRef.current?.querySelector(".palette-item.active")?.scrollIntoView({ block: "nearest" });
  }, [active]);

  function onKeyDown(event: React.KeyboardEvent) {
    if (event.key === "Escape") onClose();
    else if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((value) => Math.min(items.length - 1, value + 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((value) => Math.max(0, value - 1));
    } else if (event.key === "Enter" && items[active]) {
      event.preventDefault();
      items[active].run();
    }
  }

  const groups = Array.from(new Set(items.map((item) => item.group)));
  return (
    <div className="overlay top" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-label="Command palette" onKeyDown={onKeyDown}>
        <div className="palette-input">
          <Search size={16} />
          <input
            autoFocus
            placeholder="Search repositories, pages and actions"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            aria-label="Command"
          />
          <kbd>Esc</kbd>
        </div>
        <div className="palette-list" ref={listRef} role="listbox">
          {groups.map((group) => (
            <div key={group}>
              <div className="palette-group">{group}</div>
              {items
                .filter((item) => item.group === group)
                .map((item) => {
                  const index = items.indexOf(item);
                  return (
                    <button
                      key={`${item.group}-${item.label}`}
                      className={index === active ? "palette-item active" : "palette-item"}
                      onMouseMove={() => setActive(index)}
                      onClick={item.run}
                      role="option"
                      aria-selected={index === active}
                    >
                      {item.icon}
                      <span>{item.label}</span>
                      {index === active ? <ArrowRight size={14} className="hint" /> : item.hint && <span className="hint muted">{item.hint}</span>}
                    </button>
                  );
                })}
            </div>
          ))}
          {!items.length && <div className="empty"><p>No results for “{query}”</p></div>}
        </div>
        <div className="palette-foot">
          <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
          <span><kbd>Enter</kbd> open</span>
          <span><kbd>/</kbd> search page</span>
        </div>
      </div>
    </div>
  );
}
