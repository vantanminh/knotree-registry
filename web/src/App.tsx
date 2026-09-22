import { FormEvent, useEffect, useMemo, useState } from "react";
import { api, formatDate, Overview, Repository, RepositoryDetail, Token, User } from "./api";

type Page = "overview" | "repositories" | "tokens" | "security";
type ApiError = Error & { status?: number };

export default function App() {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    api<User>("/api/v1/auth/me")
      .then(setUser)
      .catch((reason: ApiError) => { if (reason.status !== 401) setError(reason.message); })
      .finally(() => setLoading(false));
  }, []);

  if (loading) return <div className="screen-center"><span className="spinner" /> Loading workspace</div>;
  if (!user) return <Login onLogin={setUser} error={error} />;
  return <Shell user={user} onLogout={() => setUser(null)} />;
}

function Login({ onLogin, error: initialError }: { onLogin: (user: User) => void; error: string }) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [error, setError] = useState(initialError);
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent) {
    event.preventDefault(); setBusy(true); setError("");
    try { const result = await api<{ user: User }>("/api/v1/auth/login", { method: "POST", body: JSON.stringify({ username, password }) }); onLogin(result.user); }
    catch (reason) { setError((reason as Error).message === "unauthorized" ? "The username or password is incorrect." : (reason as Error).message); }
    finally { setBusy(false); }
  }
  return <div className="auth-shell"><div className="auth-card">
    <div className="brand-mark">K</div><p className="eyebrow">PRIVATE IMAGE INFRASTRUCTURE</p><h1>Welcome back.</h1><p className="muted">Sign in to manage your repositories, credentials, and delivery surface.</p>
    <form onSubmit={submit} className="stack-lg"><label>Username<input autoComplete="username" value={username} onChange={event => setUsername(event.target.value)} required /></label><label>Password<input type="password" autoComplete="current-password" value={password} onChange={event => setPassword(event.target.value)} required /></label>{error && <div className="alert error" role="alert">{error}</div>}<button className="button primary full" disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button></form>
    <p className="tiny muted">Private by default · OCI Distribution API · R2-ready storage</p>
  </div></div>;
}

function Shell({ user, onLogout }: { user: User; onLogout: () => void }) {
  const [page, setPage] = useState<Page>("overview");
  const [menuOpen, setMenuOpen] = useState(false);
  async function logout() { await api<void>("/api/v1/auth/logout", { method: "POST" }); onLogout(); }
  const nav: [Page, string, string][] = [["overview", "Overview", "⌂"], ["repositories", "Repositories", "◈"], ["tokens", "Access tokens", "◇"], ["security", "Security", "⊙"]];
  return <div className="app-shell"><aside className={menuOpen ? "sidebar open" : "sidebar"}><div className="sidebar-top"><div className="brand-lockup"><span className="brand-mini">K</span><span>Knotree<span className="brand-soft">Registry</span></span></div><button className="close-nav" onClick={() => setMenuOpen(false)} aria-label="Close navigation">×</button></div><div className="workspace-switcher"><span className="status-dot" /> Private workspace <span className="chevron">⌄</span></div><nav aria-label="Primary navigation">{nav.map(([key, label, icon]) => <button className={page === key ? "nav-item active" : "nav-item"} key={key} onClick={() => { setPage(key); setMenuOpen(false); }}><span className="nav-icon">{icon}</span>{label}</button>)}</nav><div className="sidebar-foot"><div className="help-card"><span className="help-icon">?</span><div><strong>Need a hand?</strong><small>Read the operator guide</small></div></div><div className="user-row"><span className="avatar">{user.username.slice(0, 1).toUpperCase()}</span><div className="user-label"><strong>{user.username}</strong><small>{user.is_admin ? "Administrator" : "Member"}</small></div><button className="more" onClick={logout} title="Sign out">↪</button></div></div></aside><main className="main"><header className="topbar"><button className="menu-toggle" onClick={() => setMenuOpen(true)} aria-label="Open navigation">☰</button><div className="breadcrumbs"><span>Knotree</span><span>/</span><strong>{nav.find(item => item[0] === page)?.[1]}</strong></div><div className="top-actions"><span className="live-indicator"><i /> All systems nominal</span><button className="icon-button" title="Sign out" onClick={logout}>↪</button></div></header><div className="content"><PageContent page={page} /></div></main></div>;
}

function PageContent({ page }: { page: Page }) { if (page === "overview") return <OverviewPage />; if (page === "repositories") return <RepositoriesPage />; if (page === "tokens") return <TokensPage />; return <SecurityPage />; }

function OverviewPage() {
  const [data, setData] = useState<Overview | null>(null); const [error, setError] = useState("");
  useEffect(() => { api<Overview>("/api/v1/overview").then(setData).catch(reason => setError((reason as Error).message)); }, []);
  if (error) return <ErrorState message={error} />; if (!data) return <Loading />;
  return <><PageIntro eyebrow="WORKSPACE OVERVIEW" title={`Good to see you, ${data.user.username}.`} description="A calm view of the registry surface that matters: what is stored, who can access it, and what changed." action={<button className="button primary" onClick={() => navigator.clipboard?.writeText("docker login registry.example.com")}>Copy login example</button>} /><div className="metrics-grid"><Metric label="Repositories" value={String(data.repository_count)} detail="Private namespaces" icon="◈" /><Metric label="Active tokens" value={String(data.active_token_count)} detail="Revocable credentials" icon="◇" /><Metric label="Storage mode" value="Private" detail="R2-backed in production" icon="⬡" /></div><div className="two-col"><section className="panel"><PanelTitle title="Quick start" detail="A secure path to your first push" /><div className="steps"><Step number="01" title="Create a scoped token" text="Limit it to one repository and the actions it needs." /><Step number="02" title="Authenticate Docker" text="Use the generated secret once with docker login." /><Step number="03" title="Push by digest" text="Tags are convenient; immutable digests are your safety rail." /></div></section><section className="panel"><PanelTitle title="Your repositories" action={<span className="subtle-link">View all →</span>} />{data.repositories.length ? <div className="list">{data.repositories.slice(0, 5).map(repo => <div className="list-row" key={repo}><span className="repo-icon">◈</span><div><strong>{repo}</strong><small>Private repository</small></div><span className="row-arrow">→</span></div>)}</div> : <EmptyState title="No repositories yet" text="Push your first manifest to see it here." />}</section></div></>;
}

function RepositoriesPage() {
  const [repos, setRepos] = useState<Repository[]>([]); const [selected, setSelected] = useState<RepositoryDetail | null>(null); const [error, setError] = useState("");
  useEffect(() => { api<{ repositories: Repository[] }>("/api/v1/repositories").then(result => { setRepos(result.repositories); if (result.repositories[0]) return api<RepositoryDetail>(`/api/v1/repositories/${encodeURI(result.repositories[0].name)}`).then(setSelected); }).catch(reason => setError((reason as Error).message)); }, []);
  if (error) return <ErrorState message={error} />;
  return <><PageIntro eyebrow="REGISTRY INVENTORY" title="Repositories" description="Browse tags, immutable digests, and the media types your clients can pull." /><div className="repo-layout"><section className="panel repo-list-panel"><div className="panel-heading"><div><h2>All repositories</h2><p>{repos.length} private repositories</p></div><button className="button quiet">＋ New</button></div>{repos.length ? repos.map(repo => <button key={repo.name} className={selected?.name === repo.name ? "repo-select selected" : "repo-select"} onClick={() => api<RepositoryDetail>(`/api/v1/repositories/${encodeURI(repo.name)}`).then(setSelected)}><span className="repo-icon">◈</span><span><strong>{repo.name}</strong><small>Private</small></span><span className="row-arrow">→</span></button>) : <EmptyState title="Nothing here yet" text="Push an image to create a repository." />}</section><section className="panel repo-detail">{selected ? <><div className="detail-head"><div><p className="eyebrow">REPOSITORY</p><h2>{selected.name}</h2><span className="badge">Private</span></div><button className="button quiet" onClick={() => navigator.clipboard?.writeText(`docker pull registry.example.com/${selected.name}:latest`)}>Copy pull command</button></div><div className="table-wrap"><table><thead><tr><th>Tag</th><th>Digest</th><th>Media type</th><th>Size</th><th>Pushed</th></tr></thead><tbody>{selected.tags.length ? selected.tags.map(tag => <tr key={tag.tag}><td><span className="tag">{tag.tag}</span></td><td><code>{tag.digest.slice(0, 19)}…</code></td><td className="muted">{tag.media_type.split(".").slice(-1)[0]}</td><td className="muted">{formatBytes(tag.size)}</td><td className="muted">{formatDate(tag.created_at)}</td></tr>) : <tr><td colSpan={5}><EmptyState title="No tags yet" text="This repository has no published manifests." /></td></tr>}</tbody></table></div></> : <EmptyState title="Select a repository" text="Repository metadata will appear here." />}</section></div></>;
}

function TokensPage() {
  const [tokens, setTokens] = useState<Token[]>([]); const [secret, setSecret] = useState(""); const [formOpen, setFormOpen] = useState(false); const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const refresh = () => api<{ tokens: Token[] }>("/api/v1/auth/tokens").then(result => setTokens(result.tokens)).catch(reason => setError((reason as Error).message));
  useEffect(() => { void refresh(); }, []);
  async function create(event: FormEvent<HTMLFormElement>) { event.preventDefault(); setBusy(true); setError(""); const data = new FormData(event.currentTarget); const actions = ["pull", ...(data.get("push") ? ["push"] : []), ...(data.get("delete") ? ["delete"] : [])]; try { const result = await api<Token & { secret: string }>("/api/v1/auth/tokens", { method: "POST", body: JSON.stringify({ name: data.get("name"), scopes: [{ repository: data.get("repository"), actions }] }) }); setSecret(result.secret); setFormOpen(false); refresh(); } catch (reason) { setError((reason as Error).message); } finally { setBusy(false); } }
  async function revoke(id: string) { if (!window.confirm("Revoke this credential? Existing short-lived registry tokens remain valid only until their TTL.")) return; await api(`/api/v1/auth/tokens/${id}/revoke`, { method: "POST" }); refresh(); }
  return <><PageIntro eyebrow="CREDENTIALS" title="Access tokens" description="Create narrow, revocable credentials for Docker, CI, and deployment agents." action={<button className="button primary" onClick={() => setFormOpen(true)}>＋ Create token</button>} />{secret && <div className="alert success"><div><strong>Copy this secret now.</strong><span>It will not be shown again.</span><code>{secret}</code></div><button className="button quiet" onClick={() => navigator.clipboard?.writeText(secret)}>Copy secret</button></div>}{error && <div className="alert error">{error}</div>}{formOpen && <section className="panel form-panel"><PanelTitle title="New access token" detail="Secrets are revealed once and stored only as verifiers." /><form onSubmit={create} className="token-form"><label>Name<input name="name" placeholder="ci-production" required /></label><label>Repository<input name="repository" placeholder="team/app" required /></label><fieldset><legend>Actions</legend><label className="check"><input type="checkbox" name="pull" defaultChecked disabled /> Pull <small>Required</small></label><label className="check"><input type="checkbox" name="push" /> Push</label><label className="check"><input type="checkbox" name="delete" /> Delete</label></fieldset><div className="form-actions"><button type="button" className="button quiet" onClick={() => setFormOpen(false)}>Cancel</button><button className="button primary" disabled={busy}>{busy ? "Creating…" : "Create token"}</button></div></form></section>}<section className="panel"><PanelTitle title="Issued credentials" detail="Only prefixes and metadata are retained here." />{tokens.length ? <div className="table-wrap"><table><thead><tr><th>Name</th><th>Scope</th><th>Last used</th><th>State</th><th /></tr></thead><tbody>{tokens.map(token => <tr key={token.id}><td><strong>{token.name}</strong><small className="block">{token.prefix}…</small></td><td>{token.scopes.map(scope => <span className="scope-pill" key={scope.repository}>{scope.repository} · {scope.actions.join(", ")}</span>)}</td><td className="muted">{formatDate(token.last_used_at)}</td><td><span className={token.revoked_at ? "badge danger" : "badge green"}>{token.revoked_at ? "Revoked" : "Active"}</span></td><td>{!token.revoked_at && <button className="text-button danger-text" onClick={() => revoke(token.id)}>Revoke</button>}</td></tr>)}</tbody></table></div> : <EmptyState title="No credentials yet" text="Create a scoped token for your first Docker login." />}</section></>;
}

function SecurityPage() { const [user, setUser] = useState<User | null>(null); useEffect(() => { api<User>("/api/v1/auth/me").then(setUser); }, []); return <><PageIntro eyebrow="ACCOUNT SECURITY" title="Security" description="Keep browser access and automation credentials separate, narrow, and easy to revoke." /><div className="two-col"><section className="panel"><PanelTitle title="Account" /><div className="security-card"><span className="avatar large">{user?.username.slice(0, 1).toUpperCase() ?? "?"}</span><div><strong>{user?.username}</strong><p className="muted">{user?.is_admin ? "Administrator" : "Member"}</p></div></div><div className="setting-row"><div><strong>Password</strong><small>Use a long unique password for your browser session.</small></div><button className="button quiet" disabled>Change</button></div><div className="setting-row"><div><strong>Two-factor authentication</strong><small>Authenticator-app support is reserved for the security story.</small></div><span className="badge">Not configured</span></div></section><section className="panel"><PanelTitle title="Session hygiene" /><div className="callout"><span className="callout-icon">✓</span><div><strong>Cookie session protection is on</strong><p>HttpOnly, SameSite=Strict cookies rotate on every login. Production mode adds Secure.</p></div></div><div className="callout"><span className="callout-icon">⌁</span><div><strong>Short-lived registry tokens</strong><p>Docker Bearer tokens expire quickly. Revoking a PAT stops new tokens immediately.</p></div></div></section></div></>; }

function PageIntro({ eyebrow, title, description, action }: { eyebrow: string; title: string; description: string; action?: React.ReactNode }) { return <div className="page-intro"><div><p className="eyebrow">{eyebrow}</p><h1>{title}</h1><p className="intro-copy">{description}</p></div>{action}</div>; }
function Metric({ label, value, detail, icon }: { label: string; value: string; detail: string; icon: string }) { return <div className="metric"><span className="metric-icon">{icon}</span><div><p>{label}</p><strong>{value}</strong><small>{detail}</small></div></div>; }
function PanelTitle({ title, detail, action }: { title: string; detail?: string; action?: React.ReactNode }) { return <div className="panel-heading"><div><h2>{title}</h2>{detail && <p>{detail}</p>}</div>{action}</div>; }
function Step({ number, title, text }: { number: string; title: string; text: string }) { return <div className="step"><span>{number}</span><div><strong>{title}</strong><p>{text}</p></div></div>; }
function Loading() { return <div className="panel loading"><span className="spinner" /> Loading data…</div>; }
function ErrorState({ message }: { message: string }) { return <div className="alert error">Could not load this view: {message}</div>; }
function EmptyState({ title, text }: { title: string; text: string }) { return <div className="empty"><span className="empty-icon">◌</span><strong>{title}</strong><p>{text}</p></div>; }
function formatBytes(value: number) { if (value < 1024) return `${value} B`; if (value < 1024 * 1024) return `${Math.round(value / 1024)} KB`; return `${(value / (1024 * 1024)).toFixed(1)} MB`; }
