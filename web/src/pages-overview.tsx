import { FormEvent, useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { AuditEvent, Overview, api, friendlyError } from "./api";
import { eventLabel, formatBytes, registryPath } from "./format";
import {
  CommandBox,
  EmptyState,
  ErrorState,
  HealthDot,
  Metric,
  PageHeader,
  RelativeTime,
  RepositoryName,
  Skeleton,
  StatusBadge
} from "./ui";

export function OverviewPage({ host }: { host: string }) {
  const [data, setData] = useState<Overview | null>(null);
  const [error, setError] = useState("");
  const navigate = useNavigate();
  function load() {
    setError("");
    api<Overview>("/api/v1/overview")
      .then(setData)
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!data) return <Skeleton rows={8} />;
  const healthItems = [
    ["Registry API", data.health.status],
    ["Database", data.health.database],
    ["Object storage", data.health.storage],
    ["Auth service", data.health.status],
    ["Background jobs", data.health.status]
  ];
  return (
    <>
      <PageHeader
        title="Overview"
        description="Registry health, storage, and recent activity for this instance."
        actions={
          <button className="btn primary" onClick={() => navigate("/security/tokens?create=1")}>
            Create token
          </button>
        }
      />
      <div className="metrics">
        <Metric label="Repositories" value={String(data.repository_count)} />
        <Metric label="Storage" value={formatBytes(data.storage_bytes)} detail={`${formatBytes(data.unreferenced_bytes)} unreferenced`} />
        <Metric label="Active tokens" value={String(data.active_token_count)} />
        <Metric label="Events" value={String(data.events.length)} detail="Recent control-plane activity" />
      </div>
      <div className="grid-2">
        <section className="panel">
          <div className="panel-head">
            <div>
              <h2>Registry health</h2>
              <p>Live checks from this control plane.</p>
            </div>
            <Link className="btn sm ghost" to="/status">
              Status
            </Link>
          </div>
          <div className="health-list">
            {healthItems.map(([name, status]) => (
              <div className="health-row" key={name}>
                <HealthDot status={status} />
                <span>{name}</span>
                <span>{status === "ok" || status === "skipped" ? "Healthy" : status === "failed" || status === "missing" || status === "not_ready" ? "Failed" : status}</span>
              </div>
            ))}
          </div>
        </section>
        <section className="panel">
          <div className="panel-head">
            <div>
              <h2>Quick start</h2>
              <p>Authenticate Docker, then push an image.</p>
            </div>
          </div>
          <div className="panel-pad grid-gap">
            <CommandBox command={`docker login ${host}`} />
            <CommandBox command={`docker tag my-app ${host}/production/my-app:latest`} />
            <CommandBox command={`docker push ${host}/production/my-app:latest`} />
          </div>
        </section>
      </div>
      <div className="grid-2" style={{ marginTop: 16 }}>
        <section className="panel">
          <div className="panel-head">
            <div>
              <h2>Recent activity</h2>
              <p>Push, token, and security events.</p>
            </div>
            <Link className="btn sm ghost" to="/activity">
              View all
            </Link>
          </div>
          {data.events.length ? (
            <ActivityList events={data.events} />
          ) : (
            <EmptyState title="No activity yet" text="Pushes, logins, and credential changes will appear here." />
          )}
        </section>
        <section className="panel">
          <div className="panel-head">
            <div>
              <h2>Recently updated</h2>
              <p>Repositories with the newest tags.</p>
            </div>
            <Link className="btn sm ghost" to="/repositories">
              View all
            </Link>
          </div>
          {data.repositories.length ? (
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>Repository</th>
                    <th>Latest tag</th>
                    <th className="num">Size</th>
                    <th>Updated</th>
                  </tr>
                </thead>
                <tbody>
                  {data.repositories.slice(0, 8).map((repo) => (
                    <tr key={repo.name} onClick={() => navigate(`/repositories/${repo.name}`)}>
                      <td>
                        <RepositoryName name={repo.name} />
                      </td>
                      <td className="mono">{repo.latest_tag ?? "—"}</td>
                      <td className="num">{formatBytes(repo.size ?? 0)}</td>
                      <td>
                        <RelativeTime value={repo.updated_at} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState
              title="No repositories yet"
              text="Push your first image to start using Knotree Registry."
              action={
                <Link className="btn primary" to="/repositories">
                  View repositories
                </Link>
              }
            />
          )}
        </section>
      </div>
    </>
  );
}

export function ActivityList({ events }: { events: AuditEvent[] }) {
  return (
    <div className="timeline">
      {events.map((event) => (
        <div className="activity" key={event.id}>
          <HealthDot status={event.kind.includes("failed") ? "failed" : "ok"} />
          <div>
            <strong style={{ textTransform: "capitalize" }}>{eventLabel(event.kind)}</strong>
            <div style={{ color: "var(--muted)", fontSize: 13 }}>
              {event.repository ?? event.actor ?? "system"}
              {event.tag ? `:${event.tag}` : ""}
              {event.actor && event.repository ? ` · ${event.actor}` : ""}
            </div>
          </div>
          <time>
            <RelativeTime value={event.occurred_at} />
          </time>
        </div>
      ))}
    </div>
  );
}

export function StatusPage() {
  const [data, setData] = useState<Overview | null>(null);
  const [error, setError] = useState("");
  function load() {
    api<Overview>("/api/v1/overview")
      .then(setData)
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!data) return <Skeleton />;
  const rows = [
    ["Registry API", data.health.status],
    ["PostgreSQL", data.health.database],
    ["Object storage", data.health.storage],
    ["Background jobs", data.health.status]
  ];
  return (
    <>
      <PageHeader title="System status" description="Infrastructure checks for this registry instance." />
      <section className="panel">
        {rows.map(([name, status]) => (
          <div className="health-row" key={name}>
            <HealthDot status={status} />
            <span>{name}</span>
            <StatusBadge
              state={status === "ok" || status === "skipped" ? "ok" : status === "failed" || status === "missing" ? "fail" : "warn"}
              label={status === "ok" || status === "skipped" ? "Operational" : status}
            />
          </div>
        ))}
      </section>
      <p style={{ marginTop: 16, color: "var(--muted)" }}>Uptime {Math.floor(data.uptime_seconds / 60)} minutes.</p>
    </>
  );
}

export function ActivityPage() {
  const [events, setEvents] = useState<AuditEvent[] | null>(null);
  const [error, setError] = useState("");
  const [kind, setKind] = useState("");
  function load() {
    api<{ events: AuditEvent[] }>("/api/v1/audit?limit=200")
      .then((result) => setEvents(result.events))
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  const filtered = useMemo(
    () => (events ?? []).filter((event) => !kind || event.kind === kind),
    [events, kind]
  );
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!events) return <Skeleton />;
  const kinds = Array.from(new Set(events.map((event) => event.kind)));
  return (
    <>
      <PageHeader title="Activity" description="High-level registry events across repositories, tokens, and webhooks." />
      <div className="filters">
        <select value={kind} onChange={(event) => setKind(event.target.value)} aria-label="Filter by action">
          <option value="">All events</option>
          {kinds.map((value) => (
            <option key={value} value={value}>
              {eventLabel(value)}
            </option>
          ))}
        </select>
      </div>
      <section className="panel">
        {filtered.length ? (
          <ActivityList events={filtered} />
        ) : (
          <EmptyState title="No matching activity" text="Try a different event filter." />
        )}
      </section>
    </>
  );
}

export function LoginPage({
  onLogin
}: {
  onLogin: () => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/api/v1/auth/login", { method: "POST", body: JSON.stringify({ username, password }) });
      onLogin();
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="auth-shell">
      <div className="auth-card">
        <div className="brand-mark">K</div>
        <h1>Sign in</h1>
        <p className="lede">Knotree Registry</p>
        <form className="stack" onSubmit={submit}>
          <label className="field">
            <span>Username / Email</span>
            <input autoComplete="username" value={username} onChange={(event) => setUsername(event.target.value)} required />
          </label>
          <label className="field">
            <span>Password</span>
            <input type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} required />
          </label>
          {error && (
            <div className="alert error" role="alert">
              {error}
            </div>
          )}
          <button className="btn primary full" disabled={busy}>
            {busy ? "Signing in…" : "Sign in"}
          </button>
        </form>
      </div>
    </div>
  );
}

export function dockerLoginHint(host: string) {
  return registryPath(host, "").replace(/\/$/, "");
}
