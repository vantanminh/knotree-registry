import { useEffect, useState } from "react";
import { Link, Navigate, Outlet, Route, Routes, useLocation, useNavigate, useSearchParams } from "react-router-dom";
import {
  Activity,
  Box,
  Boxes,
  ChevronLeft,
  ChevronRight,
  KeyRound,
  LayoutDashboard,
  Menu,
  Moon,
  Settings,
  Sun,
  Terminal,
  Trash2,
  Upload,
  Users,
  Webhook,
  HardDrive,
  ScrollText,
  Cloud,
  CircleDot
} from "lucide-react";
import { Instance, User, api, friendlyError } from "./api";
import { CloudConnectionPage } from "./pages-cloud";
import { LoginPage, OverviewPage, StatusPage, ActivityPage } from "./pages-overview";
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
import { ToastProvider, useCollapsed, useTheme } from "./ui";

type NavItem = { to: string; label: string; icon: typeof LayoutDashboard; admin?: boolean };

const nav: { label: string; items: NavItem[] }[] = [
  { label: "Overview", items: [{ to: "/", label: "Overview", icon: LayoutDashboard }] },
  {
    label: "Registry",
    items: [
      { to: "/repositories", label: "Repositories", icon: Box },
      { to: "/namespaces", label: "Namespaces", icon: Boxes },
      { to: "/uploads", label: "Uploads", icon: Upload, admin: true }
    ]
  },
  {
    label: "Deployments",
    items: [
      { to: "/deployments/cloud", label: "Knotree Cloud", icon: Cloud }
    ]
  },
  {
    label: "Operations",
    items: [
      { to: "/activity", label: "Activity", icon: Activity },
      { to: "/operations/webhooks", label: "Webhooks", icon: Webhook, admin: true },
      { to: "/operations/storage", label: "Storage", icon: HardDrive },
      { to: "/operations/garbage-collection", label: "Garbage Collection", icon: Trash2, admin: true }
    ]
  },
  {
    label: "Security",
    items: [
      { to: "/security/tokens", label: "Access Tokens", icon: KeyRound },
      { to: "/namespaces/members", label: "Members", icon: Users },
      { to: "/security/audit", label: "Audit Log", icon: ScrollText }
    ]
  },
  { label: "Settings", items: [{ to: "/settings/general", label: "Settings", icon: Settings }] }
];

export default function App() {
  return (
    <ToastProvider>
      <Routes>
        <Route path="/login" element={<AuthGate />} />
        <Route element={<RequireAuth />}>
          <Route element={<Shell />}>
            <Route path="/" element={<OverviewRoute />} />
            <Route path="/repositories" element={<ReposRoute />} />
            <Route path="/repositories/*" element={<RepoDetailRoute />} />
            <Route path="/namespaces" element={<NamespacesPage />} />
            <Route path="/namespaces/members" element={<MembersPage />} />
            <Route path="/uploads" element={<UploadsRoute />} />
            <Route path="/deployments/cloud" element={<CloudRoute />} />
            <Route path="/deployments/watchers" element={<Navigate to="/deployments/cloud" replace />} />
            <Route path="/deployments/agents" element={<Navigate to="/deployments/cloud" replace />} />
            <Route path="/activity" element={<ActivityPage />} />
            <Route path="/operations/storage" element={<StoragePage />} />
            <Route path="/operations/webhooks" element={<WebhooksRoute />} />
            <Route path="/operations/garbage-collection" element={<GcRoute />} />
            <Route path="/security/tokens" element={<TokensRoute />} />
            <Route path="/security/tokens/:id" element={<TokenDetailPage />} />
            <Route path="/security/audit" element={<AuditPage />} />
            <Route path="/settings/general" element={<SettingsRoute />} />
            <Route path="/settings/registry" element={<SettingsRoute />} />
            <Route path="/settings/security" element={<AccountSecurityRoute />} />
            <Route path="/settings/retention" element={<SettingsRoute />} />
            <Route path="/account/security" element={<AccountSecurityRoute />} />
            <Route path="/status" element={<StatusPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Route>
      </Routes>
    </ToastProvider>
  );
}

function safeReturnTo(value: string | null) {
  return value?.startsWith("/") && !value.startsWith("//") && !value.includes("\\") && !/[\u0000-\u001f\u007f]/.test(value) ? value : "/";
}

function AuthGate() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const [params] = useSearchParams();
  useEffect(() => {
    api<User>("/api/v1/auth/me")
      .then(setUser)
      .catch(() => setUser(null));
  }, []);
  if (user === undefined) return <div className="auth-shell">Loading…</div>;
  if (user) return <Navigate to={safeReturnTo(params.get("returnTo"))} replace />;
  return (
    <LoginPage returnTo={safeReturnTo(params.get("returnTo"))} />
  );
}

function RequireAuth() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const location = useLocation();
  useEffect(() => {
    api<User>("/api/v1/auth/me")
      .then(setUser)
      .catch((reason) => {
        if ((reason as { status?: number }).status === 401) setUser(null);
        else setUser(null);
      });
  }, []);
  if (user === undefined) return <div className="auth-shell">Loading workspace…</div>;
  if (!user) return <Navigate to={`/login?returnTo=${encodeURIComponent(location.pathname + location.search)}`} replace />;
  return <Outlet context={{ user } satisfies { user: User }} />;
}

function Shell() {
  const [user, setUser] = useState<User | null>(null);
  const [instance, setInstance] = useState<Instance | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [palette, setPalette] = useState(false);
  const { collapsed, setCollapsed } = useCollapsed();
  const { theme, setTheme } = useTheme();
  const location = useLocation();
  const navigate = useNavigate();
  useEffect(() => {
    api<User>("/api/v1/auth/me").then(setUser).catch(() => setUser(null));
    api<Instance>("/api/v1/instance").then(setInstance).catch(() => setInstance(null));
  }, []);
  useEffect(() => {
    setMenuOpen(false);
  }, [location.pathname]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPalette(true);
      }
      if (event.key === "/" && (event.target as HTMLElement).tagName !== "INPUT" && (event.target as HTMLElement).tagName !== "TEXTAREA") {
        const search = document.querySelector<HTMLInputElement>("input[aria-label='Search repositories'], input[aria-label='Search tokens'], input[aria-label='Search...']");
        if (search) {
          event.preventDefault();
          search.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  if (!user) return <div className="auth-shell">Loading workspace…</div>;
  const host = instance?.registry_host ?? "registry.knotree.com";
  const visible = nav
    .map((group) => ({
      ...group,
      items: group.items.filter((item) => !item.admin || user.is_admin)
    }))
    .filter((group) => group.items.length);
  const crumbs = breadcrumb(location.pathname);
  async function logout() {
    await api("/api/v1/auth/logout", { method: "POST" });
    navigate("/login");
  }
  return (
    <div className="app-shell">
      <aside className={`sidebar ${collapsed ? "collapsed" : ""} ${menuOpen ? "open" : ""}`}>
        <div className="sidebar-head">
          <div className="brand">
            <span className="brand-mark" style={{ margin: 0, width: 28, height: 28, fontSize: 12 }}>
              K
            </span>
            <span className="brand-copy">
              Knotree
              <small>Registry</small>
            </span>
          </div>
          <button className="btn icon ghost close-nav" onClick={() => setMenuOpen(false)} aria-label="Close navigation">
            <ChevronLeft size={16} />
          </button>
        </div>
        <div className="nav-scroll">
          {visible.map((group) => (
            <div className="nav-group" key={group.label}>
              <div className="nav-group-label">{group.label}</div>
              {group.items.map((item) => {
                const active = item.to === "/" ? location.pathname === "/" : location.pathname.startsWith(item.to);
                return (
                  <Link key={item.to} className={active ? "nav-item active" : "nav-item"} to={item.to} title={item.label}>
                    <item.icon />
                    <span className="nav-label">{item.label}</span>
                  </Link>
                );
              })}
            </div>
          ))}
        </div>
        <div className="sidebar-foot">
          <Link className="nav-item" to="/status">
            <CircleDot />
            <span className="nav-label">Status</span>
          </Link>
          <a className="nav-item" href="https://github.com/knotree/registry" target="_blank" rel="noreferrer">
            <Terminal />
            <span className="nav-label">Documentation</span>
          </a>
          <div className="user-row">
            <span className="avatar">{user.username.slice(0, 1).toUpperCase()}</span>
            <div className="user-meta">
              <strong>{user.username}</strong>
              <span>{user.is_admin ? "Administrator" : "Member"}</span>
            </div>
            <button className="btn icon ghost" onClick={logout} title="Sign out" aria-label="Sign out">
              <ChevronRight size={16} />
            </button>
          </div>
        </div>
      </aside>
      <div className="main">
        <header className="topbar">
          <button className="btn icon ghost menu-toggle" onClick={() => setMenuOpen(true)} aria-label="Open navigation">
            <Menu size={16} />
          </button>
          <button className="btn icon ghost sidebar-toggle" onClick={() => setCollapsed(!collapsed)} aria-label="Collapse sidebar">
            {collapsed ? <ChevronRight size={16} /> : <ChevronLeft size={16} />}
          </button>
          <nav className="crumbs" aria-label="Breadcrumb">
            {crumbs.map((crumb, index) => (
              <span key={crumb.to}>
                {index > 0 && " / "}
                {index === crumbs.length - 1 ? <strong>{crumb.label}</strong> : <Link to={crumb.to}>{crumb.label}</Link>}
              </span>
            ))}
          </nav>
          <div className="top-actions">
            <span className="env-chip">
              <i className="dot ok" />
              {instance?.environment ?? "private"}
            </span>
            <button className="kbd" onClick={() => setPalette(true)}>
              ⌘ K
            </button>
            <Link className="btn primary sm" to="/security/tokens?create=1">
              Create
            </Link>
            <button
              className="btn icon ghost"
              onClick={() => setTheme(theme === "light" ? "dark" : theme === "dark" ? "system" : "light")}
              aria-label="Toggle theme"
              title={`Theme: ${theme}`}
            >
              {theme === "light" ? <Sun size={16} /> : <Moon size={16} />}
            </button>
            <Link to="/account/security" className="avatar" aria-label="Account">
              {user.username.slice(0, 1).toUpperCase()}
            </Link>
          </div>
        </header>
        <div className="content">
          <Outlet context={{ user, instance, host }} />
        </div>
      </div>
      {palette && (
        <CommandPalette
          admin={user.is_admin}
          host={host}
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

function breadcrumb(path: string): { to: string; label: string }[] {
  if (path === "/") return [{ to: "/", label: "Overview" }];
  const parts = path.split("/").filter(Boolean);
  const crumbs = [{ to: "/", label: "Knotree" }];
  let acc = "";
  for (const part of parts) {
    acc += `/${part}`;
    crumbs.push({ to: acc, label: decodeURIComponent(part) });
  }
  return crumbs;
}

function CommandPalette({
  admin,
  host,
  onClose,
  onNavigate
}: {
  admin: boolean;
  host: string;
  onClose: () => void;
  onNavigate: (path: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [repos, setRepos] = useState<string[]>([]);
  useEffect(() => {
    api<{ repositories: { name: string }[] }>("/api/v1/repositories")
      .then((result) => setRepos(result.repositories.map((repo) => repo.name)))
      .catch(() => setRepos([]));
  }, []);
  const actions = [
    { group: "Actions", label: "Create access token", to: "/security/tokens?create=1" },
    { group: "Actions", label: "Create repository", to: "/repositories" },
    { group: "Actions", label: "Open audit logs", to: "/security/audit" },
    { group: "Actions", label: "Open webhooks", to: "/operations/webhooks" },
    { group: "Actions", label: "Open settings", to: "/settings/general" },
    { group: "Actions", label: "Copy Docker login command", to: `copy:docker login ${host}` },
    ...(admin ? [{ group: "Actions", label: "Garbage collection", to: "/operations/garbage-collection" }] : [])
  ];
  const items = [
    ...repos
      .filter((name) => name.includes(query.toLowerCase()) || query.startsWith("sha256"))
      .map((name) => ({ group: "Repositories", label: name, to: `/repositories/${name}` })),
    ...actions.filter((item) => item.label.toLowerCase().includes(query.toLowerCase()) || !query)
  ];
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive((value) => Math.min(items.length - 1, value + 1));
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive((value) => Math.max(0, value - 1));
      }
      if (event.key === "Enter" && items[active]) {
        event.preventDefault();
        choose(items[active].to);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  async function choose(to: string) {
    if (to.startsWith("copy:")) {
      await navigator.clipboard.writeText(to.slice(5));
      onClose();
      return;
    }
    onNavigate(to);
  }
  const groups = Array.from(new Set(items.map((item) => item.group)));
  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-label="Command palette">
        <input autoFocus placeholder="Search commands or resources" value={query} onChange={(event) => { setQuery(event.target.value); setActive(0); }} />
        <div className="palette-list">
          {groups.map((group) => (
            <div key={group}>
              <div className="group-title">{group}</div>
              {items
                .filter((item) => item.group === group)
                .map((item) => {
                  const index = items.indexOf(item);
                  return (
                    <button key={item.to} className={index === active ? "palette-item active" : "palette-item"} onClick={() => choose(item.to)}>
                      {item.label}
                      <small>{item.to.startsWith("copy:") ? "Copy" : "Open"}</small>
                    </button>
                  );
                })}
            </div>
          ))}
          {!items.length && <div className="empty">No matching commands</div>}
        </div>
      </div>
    </div>
  );
}

function OverviewRoute() {
  const host = useHost();
  return <OverviewPage host={host} />;
}
function ReposRoute() {
  return <RepositoriesPage host={useHost()} />;
}
function RepoDetailRoute() {
  return <RepositoryDetailPage host={useHost()} />;
}
function TokensRoute() {
  const user = useSessionUser();
  return <TokensPage host={useHost()} username={user.username} />;
}
function UploadsRoute() {
  return <UploadsPage admin={useSessionUser().is_admin} />;
}
function CloudRoute() {
  return <CloudConnectionPage admin={useSessionUser().is_admin} host={useHost()} />;
}
function WebhooksRoute() {
  return <WebhooksPage admin={useSessionUser().is_admin} />;
}
function GcRoute() {
  return <GarbageCollectionPage admin={useSessionUser().is_admin} />;
}
function SettingsRoute() {
  const [instance, setInstance] = useState<Instance | null>(null);
  useEffect(() => {
    api<Instance>("/api/v1/instance").then(setInstance).catch(() => setInstance(null));
  }, []);
  return <SettingsPage instance={instance} />;
}
function AccountSecurityRoute() {
  return <SecuritySettingsPage user={useSessionUser()} />;
}

function useHost(): string {
  const [host, setHost] = useState("registry.knotree.com");
  useEffect(() => {
    api<Instance>("/api/v1/instance")
      .then((instance) => setHost(instance.registry_host))
      .catch(() => undefined);
  }, []);
  return host;
}

function useSessionUser(): User {
  const [user, setUser] = useState<User>({ id: "", username: "", is_admin: false });
  useEffect(() => {
    api<User>("/api/v1/auth/me").then(setUser);
  }, []);
  return user;
}
