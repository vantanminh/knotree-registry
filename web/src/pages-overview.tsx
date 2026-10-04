import { useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  Activity,
  ArrowRight,
  ArrowUpRight,
  Box,
  CheckCircle2,
  Clock,
  Database,
  HardDrive,
  KeyRound,
  Layers,
  ShieldCheck,
  Tags,
  XCircle,
  Zap
} from "lucide-react";
import { AuditEvent, Overview, useResource } from "./api";
import { ActivityFeed, dailyCounts, eventMeta, eventResource } from "./events";
import { formatBytes, formatDuration, splitBytes } from "./format";
import { useSession } from "./session";
import {
  CommandBox,
  DigestView,
  Drawer,
  EmptyState,
  ErrorState,
  HealthDot,
  Logo,
  Meter,
  PageHeader,
  Panel,
  RelativeTime,
  RepositoryName,
  Segmented,
  Skeleton,
  Stats,
  StatusBadge,
  TagPill,
  healthLabel,
  healthTone,
  useCopy
} from "./ui";

function healthRows(data: Overview) {
  return [
    { name: "Registry API", detail: "OCI distribution + control plane", status: data.health.status, icon: <Zap size={15} /> },
    { name: "Database", detail: "Metadata, tokens, audit", status: data.health.database, icon: <Database size={15} /> },
    { name: "Object storage", detail: "Blobs and manifests", status: data.health.storage, icon: <HardDrive size={15} /> }
  ];
}

export function OverviewPage() {
  const { host, user } = useSession();
  const overview = useResource<Overview>("/api/v1/overview");
  const audit = useResource<{ events: AuditEvent[] }>("/api/v1/audit?limit=200");
  const navigate = useNavigate();
  const copy = useCopy();
  const [range, setRange] = useState<"14" | "30">("14");
  const data = overview.data;
  const events = audit.data?.events ?? data?.events ?? [];
  const buckets = useMemo(() => dailyCounts(events, Number(range)), [events, range]);

  if (overview.error) return <ErrorState message={overview.error} onRetry={overview.reload} />;
  if (!data) return <Skeleton rows={6} stats />;

  const rows = healthRows(data);
  const allOk = rows.every((row) => healthTone(row.status) === "ok");
  const tagCount = data.repositories.reduce((sum, repo) => sum + (repo.tag_count ?? 0), 0);
  const pushes = buckets.reduce((sum, bucket) => sum + bucket.pushes, 0);
  const recent = [...data.repositories].sort((a, b) => (b.updated_at ?? 0) - (a.updated_at ?? 0)).slice(0, 6);
  const largest = [...data.repositories].sort((a, b) => (b.size ?? 0) - (a.size ?? 0)).slice(0, 5);
  const maxSize = largest[0]?.size ?? 0;
  const unrefShare = data.storage_bytes ? data.unreferenced_bytes / data.storage_bytes : 0;

  return (
    <>
      <PageHeader
        eyebrow={
          <>
            <span className={`dot ${allOk ? "ok" : "warn"}`} style={{ width: 6, height: 6, flexBasis: 6, boxShadow: "none" }} />
            {allOk ? "All systems operational" : "Degraded — check system status"}
          </>
        }
        title="Overview"
        description={
          <>
            Images, credentials and delivery for <span className="mono" style={{ color: "var(--text)" }}>{host}</span>.
          </>
        }
        actions={
          <>
            <button className="btn" onClick={() => copy(`docker login ${host}`, "Login command copied")}>
              <KeyRound size={14} /> Copy docker login
            </button>
            <Link className="btn primary" to="/repositories">
              Browse repositories <ArrowRight size={14} />
            </Link>
          </>
        }
      />

      <Stats
        items={[
          {
            label: "Repositories",
            icon: <Box />,
            value: data.repository_count,
            detail: <Link to="/namespaces">{new Set(data.repositories.map((r) => r.name.split("/")[0])).size} namespaces</Link>
          },
          { label: "Tags", icon: <Tags />, value: tagCount, detail: `${pushes} pushes in ${range} days` },
          {
            label: "Stored",
            icon: <HardDrive />,
            ...splitBytes(data.storage_bytes),
            detail: data.unreferenced_bytes ? `${formatBytes(data.unreferenced_bytes)} reclaimable` : "Nothing to reclaim"
          },
          { label: "Active tokens", icon: <KeyRound />, value: data.active_token_count, detail: <Link to="/security/tokens">Manage access</Link> },
          { label: "Uptime", icon: <Clock />, value: formatDuration(data.uptime_seconds), detail: "Since last restart" }
        ]}
      />

      <div className="grid">
        <Panel
          className="span-8"
          title="Registry activity"
          description="Pushes and tag updates against all other control-plane events."
          actions={
            <Segmented
              label="Range"
              value={range}
              onChange={setRange}
              options={[
                { id: "14", label: "14d" },
                { id: "30", label: "30d" }
              ]}
            />
          }
        >
          <div className="panel-body">
            <ActivityChart buckets={buckets} />
          </div>
        </Panel>

        <Panel
          className="span-4"
          title="System health"
          description="Live readiness checks."
          actions={
            <Link className="btn sm ghost" to="/status">
              Details <ArrowUpRight size={13} />
            </Link>
          }
        >
          <div className="health">
            {rows.map((row) => (
              <div className="health-row" key={row.name}>
                <span className="muted" style={{ display: "grid" }}>{row.icon}</span>
                <span className="name">
                  {row.name}
                  <div className="muted" style={{ fontSize: 12 }}>{row.detail}</div>
                </span>
                <span className="row" style={{ gap: 8 }}>
                  <span className="state">{healthLabel(row.status)}</span>
                  <HealthDot status={row.status} />
                </span>
              </div>
            ))}
          </div>
        </Panel>

        <Panel
          className="span-8"
          title="Recently pushed"
          actions={
            <Link className="btn sm ghost" to="/repositories">
              All repositories <ArrowRight size={13} />
            </Link>
          }
        >
          {recent.length ? (
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>Repository</th>
                    <th>Latest tag</th>
                    <th className="hide-sm">Digest</th>
                    <th className="num">Size</th>
                    <th className="num">Updated</th>
                  </tr>
                </thead>
                <tbody>
                  {recent.map((repo) => (
                    <tr key={repo.name} className="clickable" onClick={() => navigate(`/repositories/${repo.name}`)}>
                      <td>
                        <RepositoryName name={repo.name} />
                      </td>
                      <td>
                        <TagPill tag={repo.latest_tag} />
                      </td>
                      <td className="hide-sm">
                        <DigestView value={repo.latest_digest} />
                      </td>
                      <td className="num">{formatBytes(repo.size ?? 0)}</td>
                      <td className="num muted">
                        <RelativeTime value={repo.updated_at} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="panel-body">
              <EmptyState
                icon={<Box size={18} />}
                title="No images yet"
                text="Your first push creates the repository. Run these from any machine with Docker."
              />
              <div className="stack-sm" style={{ maxWidth: 560, margin: "0 auto 12px" }}>
                <CommandBox command={`docker login ${host}`} />
                <CommandBox command={`docker push ${host}/team/app:latest`} />
              </div>
            </div>
          )}
        </Panel>

        <Panel
          className="span-4"
          title="Storage"
          actions={
            <Link className="btn sm ghost" to="/operations/storage">
              Breakdown <ArrowUpRight size={13} />
            </Link>
          }
        >
          <div className="panel-body">
            <div className="row" style={{ alignItems: "baseline", gap: 6, marginBottom: 12 }}>
              <span style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-0.03em" }} className="tnum">
                {formatBytes(data.storage_bytes)}
              </span>
              <span className="muted" style={{ fontSize: 12.5 }}>in object storage</span>
            </div>
            <Meter
              parts={[
                { label: "Referenced", value: data.referenced_bytes, color: "var(--text-2)" },
                { label: "Unreferenced", value: data.unreferenced_bytes, color: "var(--accent)" }
              ]}
            />
            <div className="meter-legend">
              <div>
                <i style={{ background: "var(--text-2)" }} />
                <span>Referenced by tags</span>
                <span>{formatBytes(data.referenced_bytes)}</span>
              </div>
              <div>
                <i style={{ background: "var(--accent)" }} />
                <span>Unreferenced</span>
                <span>{formatBytes(data.unreferenced_bytes)}</span>
              </div>
            </div>
            {unrefShare > 0.15 && user.is_admin && (
              <Link className="btn sm full" style={{ marginTop: 14 }} to="/operations/garbage-collection">
                Reclaim {Math.round(unrefShare * 100)}% with garbage collection
              </Link>
            )}
            {largest.length > 0 && (
              <>
                <div className="section-title" style={{ marginTop: 20 }}>Largest repositories</div>
                <div className="rank">
                  {largest.map((repo) => (
                    <Link className="rank-row" key={repo.name} to={`/repositories/${repo.name}`}>
                      <span className="name mono" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{repo.name}</span>
                      <span className="value">{formatBytes(repo.size ?? 0)}</span>
                      <span className="track">
                        <span style={{ width: `${maxSize ? Math.max(2, ((repo.size ?? 0) / maxSize) * 100) : 0}%` }} />
                      </span>
                    </Link>
                  ))}
                </div>
              </>
            )}
          </div>
        </Panel>

        <Panel
          className="span-8"
          title="Recent activity"
          actions={
            <Link className="btn sm ghost" to="/activity">
              View all <ArrowRight size={13} />
            </Link>
          }
        >
          {data.events.length ? (
            <div style={{ paddingBottom: 8 }}>
              <ActivityFeed events={data.events.slice(0, 8)} />
            </div>
          ) : (
            <EmptyState icon={<Activity size={18} />} title="No activity yet" text="Pushes, sign-ins and credential changes will show up here." />
          )}
        </Panel>

        <Panel className="span-4" title="Push an image" description="Authenticate with an access token as the password.">
          <div className="panel-body">
            <ol className="steps">
              <li>
                <div>
                  <strong>Sign in to the registry</strong>
                  <CommandBox command={`docker login ${host} -u ${user.username}`} />
                </div>
              </li>
              <li>
                <div>
                  <strong>Tag your image</strong>
                  <CommandBox command={`docker tag app ${host}/team/app:1.0`} />
                </div>
              </li>
              <li>
                <div>
                  <strong>Push</strong>
                  <CommandBox command={`docker push ${host}/team/app:1.0`} />
                </div>
              </li>
            </ol>
          </div>
          <div className="panel-foot">
            <span>Need a password?</span>
            <Link className="btn sm" to="/security/tokens?create=1">
              <KeyRound size={13} /> Create token
            </Link>
          </div>
        </Panel>
      </div>
    </>
  );
}

function ActivityChart({ buckets }: { buckets: ReturnType<typeof dailyCounts> }) {
  const max = Math.max(1, ...buckets.map((bucket) => bucket.pushes + bucket.other));
  const total = buckets.reduce((sum, bucket) => sum + bucket.pushes + bucket.other, 0);
  const fmt = (date: Date) => date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  return (
    <>
      <div className="row" style={{ justifyContent: "space-between", marginBottom: 4 }}>
        <div className="chart-legend">
          <span><i style={{ background: "var(--accent)" }} />Pushes</span>
          <span><i style={{ background: "color-mix(in srgb, var(--text) 22%, transparent)" }} />Other events</span>
        </div>
        <span className="muted tnum" style={{ fontSize: 12.5 }}>{total} events</span>
      </div>
      <div className="bars" role="img" aria-label={`${total} events over ${buckets.length} days`}>
        {buckets.map((bucket) => {
          const sum = bucket.pushes + bucket.other;
          return (
            <div className="bar-col" key={bucket.date.toISOString()}>
              <span className="tip">
                {fmt(bucket.date)} · {bucket.pushes} push{bucket.pushes === 1 ? "" : "es"}, {bucket.other} other
              </span>
              {sum === 0 ? (
                <div className="bar zero" />
              ) : (
                <>
                  {bucket.pushes > 0 && <div className="bar" style={{ height: `${(bucket.pushes / max) * 100}%` }} />}
                  {bucket.other > 0 && <div className="bar secondary" style={{ height: `${(bucket.other / max) * 100}%` }} />}
                </>
              )}
            </div>
          );
        })}
      </div>
      <div className="bars-axis">
        <span>{fmt(buckets[0].date)}</span>
        <span>{fmt(buckets[Math.floor(buckets.length / 2)].date)}</span>
        <span>Today</span>
      </div>
    </>
  );
}

export function StatusPage() {
  const { data, error, reload } = useResource<Overview>("/api/v1/overview");
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton rows={4} />;
  const rows = healthRows(data);
  const tones = rows.map((row) => healthTone(row.status));
  const worst = tones.includes("fail") ? "fail" : tones.includes("warn") ? "warn" : "ok";
  return (
    <>
      <PageHeader
        title="System status"
        description="Readiness of the services this registry depends on."
        actions={
          <button className="btn" onClick={reload}>
            Refresh
          </button>
        }
      />
      <section className="panel" style={{ marginBottom: 16 }}>
        <div className="status-hero">
          <span className={`big-dot ${worst}`}>{worst === "ok" ? <CheckCircle2 size={20} /> : <XCircle size={20} />}</span>
          <div>
            <h2>{worst === "ok" ? "All systems operational" : worst === "warn" ? "Some systems are degraded" : "A dependency is down"}</h2>
            <p>
              Up for {formatDuration(data.uptime_seconds)} · checked <RelativeTime value={Math.floor(Date.now() / 1000)} />
            </p>
          </div>
        </div>
      </section>
      <Panel title="Components">
        <div className="health">
          {rows.map((row) => (
            <div className="health-row" key={row.name} style={{ padding: "14px 18px" }}>
              <span className="muted" style={{ display: "grid" }}>{row.icon}</span>
              <span className="name">
                <strong style={{ fontWeight: 500 }}>{row.name}</strong>
                <div className="muted" style={{ fontSize: 12.5 }}>{row.detail}</div>
              </span>
              <StatusBadge dot state={healthTone(row.status)} label={healthLabel(row.status)} />
            </div>
          ))}
        </div>
      </Panel>
    </>
  );
}

export function ActivityPage() {
  const { data, error, reload } = useResource<{ events: AuditEvent[] }>("/api/v1/audit?limit=200");
  const [kind, setKind] = useState("");
  const [selected, setSelected] = useState<AuditEvent | null>(null);
  const events = data?.events ?? [];
  const counts = useMemo(() => {
    const map = new Map<string, number>();
    for (const event of events) map.set(event.kind, (map.get(event.kind) ?? 0) + 1);
    return [...map.entries()].sort((a, b) => b[1] - a[1]);
  }, [events]);
  const filtered = events.filter((event) => !kind || event.kind === kind);
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton />;
  return (
    <>
      <PageHeader title="Activity" description="Everything that happened on this registry, newest first. The last 200 events are kept here." />
      {counts.length > 0 && (
        <div className="toolbar">
          <div className="filter-chips">
            <button className={!kind ? "filter-chip active" : "filter-chip"} onClick={() => setKind("")}>
              All <span className="count">{events.length}</span>
            </button>
            {counts.map(([value, count]) => (
              <button key={value} className={kind === value ? "filter-chip active" : "filter-chip"} onClick={() => setKind(kind === value ? "" : value)}>
                {eventMeta(value).verb} <span className="count">{count}</span>
              </button>
            ))}
          </div>
        </div>
      )}
      <section className="panel" style={{ paddingBottom: 8 }}>
        {filtered.length ? (
          <ActivityFeed events={filtered} onSelect={setSelected} />
        ) : (
          <EmptyState icon={<Activity size={18} />} title="No activity yet" text="Pushes, sign-ins and credential changes will appear here." />
        )}
      </section>
      {selected && <EventDrawer event={selected} onClose={() => setSelected(null)} />}
    </>
  );
}

export function EventDrawer({ event, onClose }: { event: AuditEvent; onClose: () => void }) {
  const info = eventMeta(event.kind);
  const resource = eventResource(event);
  return (
    <Drawer eyebrow={<span className="mono">{event.kind}</span>} title={info.verb} onClose={onClose}>
      <dl className="kv">
        <dt>Result</dt>
        <dd>
          <StatusBadge dot state={event.kind.includes("failed") ? "fail" : "ok"} label={event.kind.includes("failed") ? "Failed" : "Succeeded"} />
        </dd>
        <dt>Actor</dt>
        <dd>{event.actor ?? "system"}</dd>
        <dt>When</dt>
        <dd>
          <RelativeTime value={event.occurred_at} /> <span className="muted">· {new Date(event.occurred_at * 1000).toLocaleString()}</span>
        </dd>
        {resource && (
          <>
            <dt>Resource</dt>
            <dd>
              {event.repository ? (
                <Link className="inline-link mono" to={`/repositories/${event.repository}`}>
                  {resource}
                </Link>
              ) : (
                resource
              )}
            </dd>
          </>
        )}
        {event.digest && (
          <>
            <dt>Digest</dt>
            <dd>
              <DigestView value={event.digest} />
            </dd>
          </>
        )}
        <dt>Event ID</dt>
        <dd className="mono muted">{event.id}</dd>
      </dl>
      <div>
        <div className="section-title">Metadata</div>
        <pre className="code-block">{JSON.stringify(event.metadata ?? {}, null, 2)}</pre>
      </div>
    </Drawer>
  );
}

/** One Knotree account signs in to every Knotree service. */
export function LoginPage({ returnTo = "/" }: { returnTo?: string }) {
  const start = (intent?: string) =>
    `/api/v1/auth/sso/start?returnTo=${encodeURIComponent(returnTo)}${intent ? `&intent=${intent}` : ""}`;
  return (
    <div className="auth-shell">
      <div className="auth-main">
        <div className="wordmark">
          <Logo />
          Knotree Registry
        </div>
        <div className="auth-form">
          <h1>Sign in to your registry</h1>
          <p className="lede">Use your Knotree account. The same sign-in works across Cloud, Accounts and Registry.</p>
          <div className="stack">
            <a className="btn primary lg full" href={start()}>
              Continue with Knotree
            </a>
            <a className="btn lg full" href={start("signup")}>
              Create a Knotree account
            </a>
          </div>
          <p className="fineprint">
            Pushing from a terminal or CI? Sign in once, then create an access token to use as your <code>docker login</code> password.
          </p>
        </div>
        <div className="auth-foot">
          <span>© Knotree</span>
          <a href="https://github.com/knotree/registry" target="_blank" rel="noreferrer">
            Documentation
          </a>
        </div>
      </div>
      <aside className="auth-aside" aria-hidden>
        <h2>
          A private OCI registry that ships the <em>exact digest</em> you pushed.
        </h2>
        <div className="points">
          <div><ShieldCheck size={15} /> Scoped, revocable tokens for every pipeline and server.</div>
          <div><Layers size={15} /> Content-addressed storage with safe, previewable cleanup.</div>
          <div><Zap size={15} /> Signed tag events trigger Knotree Cloud deploys automatically.</div>
        </div>
        <div className="terminal">
          <div className="terminal-bar"><i /><i /><i /></div>
          <div className="terminal-body">
            <span className="p">$ </span>docker push registry.knotree.com/team/api:1.4.2{"\n"}
            <span className="dim">The push refers to repository [registry.knotree.com/team/api]</span>{"\n"}
            <span className="dim">5f70bf18a086: </span><span className="ok">Pushed</span>{"\n"}
            <span className="dim">a3ed95caeb02: </span><span className="ok">Pushed</span>{"\n"}
            <span className="dim">1.4.2: digest: </span><span className="acc">sha256:9b2c41e07d5a</span><span className="dim">… size: 1572</span>
          </div>
        </div>
      </aside>
    </div>
  );
}

