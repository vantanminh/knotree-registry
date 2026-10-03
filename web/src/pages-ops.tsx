import { FormEvent, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import {
  ArrowUpRight,
  Box,
  CheckCircle2,
  ExternalLink,
  HardDrive,
  Layers,
  Lock,
  Plus,
  ScrollText,
  ShieldCheck,
  Trash2,
  Upload,
  Users,
  Webhook as WebhookIcon
} from "lucide-react";
import { AuditEvent, GcReport, StorageOverview, Token, UploadSession, Webhook, api, friendlyError, useResource } from "./api";
import { eventFailed, eventMeta, groupByDay } from "./events";
import { formatBytes, formatDuration, splitBytes } from "./format";
import { EventDrawer } from "./pages-overview";
import { useSession } from "./session";
import {
  Callout,
  ConfirmDialog,
  CopyButton,
  EmptyState,
  ErrorState,
  Meter,
  Modal,
  PageHeader,
  Panel,
  RelativeTime,
  RepositoryName,
  Skeleton,
  Stats,
  StatusBadge,
  useToast
} from "./ui";

function NoAccess() {
  return (
    <EmptyState
      icon={<Lock size={18} />}
      title="Administrators only"
      text="Ask an administrator of this registry if you need access to this page."
      action={
        <Link className="btn" to="/">
          Back to overview
        </Link>
      }
    />
  );
}

/* ---------- audit ---------- */

export function AuditPage() {
  const { data, error, reload } = useResource<{ events: AuditEvent[] }>("/api/v1/audit?limit=200");
  const [params, setParams] = useSearchParams();
  const [selected, setSelected] = useState<AuditEvent | null>(null);
  const action = params.get("action") ?? "";
  const actor = params.get("actor") ?? "";
  const repository = params.get("repository") ?? "";
  const result = params.get("result") ?? "";
  const events = data?.events;
  const filtered = useMemo(
    () =>
      (events ?? []).filter((event) => {
        if (action && event.kind !== action) return false;
        if (actor && (event.actor ?? "") !== actor) return false;
        if (repository && (event.repository ?? "") !== repository) return false;
        if (result === "failed" && !eventFailed(event.kind)) return false;
        if (result === "success" && eventFailed(event.kind)) return false;
        return true;
      }),
    [events, action, actor, repository, result]
  );
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!events) return <Skeleton />;
  const actions = Array.from(new Set(events.map((event) => event.kind))).sort();
  const actors = (Array.from(new Set(events.map((event) => event.actor).filter(Boolean))) as string[]).sort();
  const repos = (Array.from(new Set(events.map((event) => event.repository).filter(Boolean))) as string[]).sort();
  const failures = events.filter((event) => eventFailed(event.kind)).length;
  const active = [action, actor, repository, result].filter(Boolean).length;
  function set(key: string, value: string) {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    setParams(next, { replace: true });
  }
  const groups = groupByDay(filtered, (event) => event.occurred_at);
  return (
    <>
      <PageHeader
        title="Audit log"
        description="Security-relevant events: sign-ins, credentials, pushes, deletions and webhook changes."
        badge={failures > 0 ? <StatusBadge state="fail" dot label={`${failures} failed`} /> : undefined}
      />
      <div className="toolbar">
        <select className="select-sm" value={actor} onChange={(event) => set("actor", event.target.value)} aria-label="Actor">
          <option value="">Any actor</option>
          {actors.map((value) => (
            <option key={value}>{value}</option>
          ))}
        </select>
        <select className="select-sm" value={action} onChange={(event) => set("action", event.target.value)} aria-label="Action">
          <option value="">Any action</option>
          {actions.map((value) => (
            <option key={value} value={value}>
              {eventMeta(value).verb}
            </option>
          ))}
        </select>
        <select className="select-sm" value={repository} onChange={(event) => set("repository", event.target.value)} aria-label="Repository">
          <option value="">Any repository</option>
          {repos.map((value) => (
            <option key={value}>{value}</option>
          ))}
        </select>
        <select className="select-sm" value={result} onChange={(event) => set("result", event.target.value)} aria-label="Result">
          <option value="">Any result</option>
          <option value="success">Succeeded</option>
          <option value="failed">Failed</option>
        </select>
        {active > 0 && (
          <button className="btn sm ghost" onClick={() => setParams({}, { replace: true })}>
            Clear {active}
          </button>
        )}
        <span className="spacer" />
        <span className="result-count">
          {filtered.length} of {events.length} events
        </span>
      </div>
      <section className="panel">
        {filtered.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Event</th>
                  <th>Actor</th>
                  <th className="hide-sm">Resource</th>
                  <th>Result</th>
                  <th className="num">Time</th>
                </tr>
              </thead>
              <tbody>
                {groups.map((group) => (
                  <GroupRows key={group.day} day={group.day} events={group.items} onSelect={setSelected} />
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState
            icon={<ScrollText size={18} />}
            title={events.length ? "No matching events" : "No events yet"}
            text={events.length ? "Try removing a filter." : "Security-sensitive activity will appear here."}
          />
        )}
      </section>
      {selected && <EventDrawer event={selected} onClose={() => setSelected(null)} />}
    </>
  );
}

function GroupRows({ day, events, onSelect }: { day: string; events: AuditEvent[]; onSelect: (event: AuditEvent) => void }) {
  return (
    <>
      <tr className="group-row">
        <td colSpan={5}>{day}</td>
      </tr>
      {events.map((event) => {
        const info = eventMeta(event.kind);
        const failed = eventFailed(event.kind);
        return (
          <tr key={event.id} className="clickable" onClick={() => onSelect(event)} tabIndex={0} onKeyDown={(e) => e.key === "Enter" && onSelect(event)}>
            <td>
              <span className="row" style={{ gap: 10, flexWrap: "nowrap" }}>
                <span className={`feed-icon ${info.tone}`} style={{ width: 24, height: 24 }}>{info.icon}</span>
                {info.verb}
              </span>
            </td>
            <td>{event.actor ?? <span className="muted">system</span>}</td>
            <td className="mono hide-sm" style={{ color: "var(--text-2)" }}>
              {event.repository ? `${event.repository}${event.tag ? `:${event.tag}` : ""}` : <span className="muted">—</span>}
            </td>
            <td>
              <StatusBadge dot state={failed ? "fail" : "ok"} label={failed ? "Failed" : "OK"} />
            </td>
            <td className="num muted">{new Date(event.occurred_at * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}</td>
          </tr>
        );
      })}
    </>
  );
}

/* ---------- storage ---------- */

export function StoragePage() {
  const { instance, user } = useSession();
  const { data, error, reload } = useResource<StorageOverview>("/api/v1/storage");
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton stats />;
  const sorted = [...data.repositories].sort((left, right) => (right.size ?? 0) - (left.size ?? 0));
  const share = (bytes: number) => (data.total_bytes ? Math.round((bytes / data.total_bytes) * 1000) / 10 : 0);
  return (
    <>
      <PageHeader
        title="Storage"
        description="What's in object storage, and which repositories use the most."
        actions={
          user.is_admin && data.unreferenced_bytes > 0 ? (
            <Link className="btn" to="/operations/garbage-collection">
              <Trash2 size={14} /> Reclaim {formatBytes(data.unreferenced_bytes)}
            </Link>
          ) : undefined
        }
      />
      <Stats
        items={[
          { label: "Total stored", icon: <HardDrive />, ...splitBytes(data.total_bytes) },
          { label: "Referenced", icon: <Layers />, ...splitBytes(data.referenced_bytes), detail: `${share(data.referenced_bytes)}% of total` },
          { label: "Unreferenced", icon: <Trash2 />, ...splitBytes(data.unreferenced_bytes), detail: `${share(data.unreferenced_bytes)}% · reclaimable` },
          { label: "Repositories", icon: <Box />, value: data.repository_count }
        ]}
      />
      <div className="grid">
        <Panel className="span-8" title="By repository" description="Sorted by stored size.">
          {sorted.length ? (
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>Repository</th>
                    <th className="num">Size</th>
                    <th className="num hide-sm">Share</th>
                    <th className="num hide-sm">Manifests</th>
                    <th className="num hide-sm">Tags</th>
                    <th className="num">Last push</th>
                  </tr>
                </thead>
                <tbody>
                  {sorted.map((repo) => (
                    <tr key={repo.name}>
                      <td>
                        <Link to={`/repositories/${repo.name}`}>
                          <RepositoryName name={repo.name} />
                        </Link>
                      </td>
                      <td className="num">{formatBytes(repo.size ?? 0)}</td>
                      <td className="num hide-sm">
                        <span className="inline-meter">
                          <span className="tnum muted">{share(repo.size ?? 0)}%</span>
                          <span className="track">
                            <span style={{ width: `${share(repo.size ?? 0)}%` }} />
                          </span>
                        </span>
                      </td>
                      <td className="num hide-sm">{repo.manifest_count ?? 0}</td>
                      <td className="num hide-sm">{repo.tag_count ?? 0}</td>
                      <td className="num muted">
                        <RelativeTime value={repo.updated_at} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState icon={<HardDrive size={18} />} title="Nothing stored yet" text="Push an image to start using storage." />
          )}
        </Panel>
        <div className="span-4 stack-lg">
          <Panel title="Composition">
            <div className="panel-body">
              <Meter
                parts={[
                  { label: "Referenced", value: data.referenced_bytes, color: "var(--text-2)" },
                  { label: "Unreferenced", value: data.unreferenced_bytes, color: "var(--accent)" }
                ]}
              />
              <div className="meter-legend">
                <div>
                  <i style={{ background: "var(--text-2)" }} />
                  <span>Referenced by a tag</span>
                  <span>{formatBytes(data.referenced_bytes)}</span>
                </div>
                <div>
                  <i style={{ background: "var(--accent)" }} />
                  <span>Unreferenced</span>
                  <span>{formatBytes(data.unreferenced_bytes)}</span>
                </div>
              </div>
            </div>
          </Panel>
          {instance && (
            <Panel title="Backend">
              <div className="panel-body">
                <dl className="kv compact">
                  <dt>Provider</dt>
                  <dd>{instance.storage_backend === "r2" ? "Cloudflare R2" : instance.storage_backend}</dd>
                  <dt>Bucket</dt>
                  <dd className="mono">{instance.storage_bucket ?? "local"}</dd>
                  <dt>Blob serving</dt>
                  <dd className="mono">{instance.pull_mode}</dd>
                </dl>
              </div>
            </Panel>
          )}
        </div>
      </div>
    </>
  );
}

/* ---------- garbage collection ---------- */

export function GarbageCollectionPage() {
  const { user } = useSession();
  const storage = useResource<StorageOverview>(user.is_admin ? "/api/v1/storage" : null);
  const [report, setReport] = useState<GcReport | null>(null);
  const [busy, setBusy] = useState<"dry" | "run" | null>(null);
  const [confirm, setConfirm] = useState(false);
  const toast = useToast();
  if (!user.is_admin) return <NoAccess />;
  if (storage.error) return <ErrorState message={storage.error} onRetry={storage.reload} />;
  if (!storage.data) return <Skeleton />;
  async function run(dry_run: boolean) {
    setBusy(dry_run ? "dry" : "run");
    try {
      const result = await api<GcReport>("/api/v1/admin/gc", { method: "POST", body: JSON.stringify({ dry_run }) });
      setReport(result);
      toast(dry_run ? "Dry run complete — nothing was deleted" : `Reclaimed ${formatBytes(result.reclaimed_bytes)}`);
      storage.reload();
    } catch (reason) {
      toast(friendlyError(reason), "error");
    } finally {
      setBusy(null);
      setConfirm(false);
    }
  }
  const previewed = report?.dry_run;
  return (
    <>
      <PageHeader
        title="Garbage collection"
        description="Delete blobs and manifests no tag points to anymore. Preview first; a run only removes content past the grace period."
      />
      <Stats
        items={[
          { label: "Unreferenced now", icon: <Trash2 />, ...splitBytes(storage.data.unreferenced_bytes) },
          { label: "Total stored", icon: <HardDrive />, ...splitBytes(storage.data.total_bytes) },
          {
            label: report ? (report.dry_run ? "Would reclaim" : "Reclaimed") : "Would reclaim",
            icon: <CheckCircle2 />,
            value: report ? formatBytes(report.reclaimed_bytes).split(" ")[0] : "—",
            unit: report ? formatBytes(report.reclaimed_bytes).split(" ")[1] : undefined,
            detail: report ? `${report.blobs} blobs · ${report.manifests} manifests` : "Run a preview to find out"
          }
        ]}
      />
      <Panel title="Run" description="Two steps. The preview is read-only.">
        <div className="panel-body">
          <ol className="steps">
            <li>
              <div>
                <strong>Preview</strong>
                <p>Lists what would be deleted. Nothing changes.</p>
                <button className="btn" disabled={busy !== null} onClick={() => run(true)}>
                  {busy === "dry" ? "Scanning…" : previewed ? "Preview again" : "Run preview"}
                </button>
              </div>
            </li>
            <li>
              <div>
                <strong>Collect</strong>
                <p>Permanently deletes the content from object storage. Images still referenced by a tag are never touched.</p>
                <button className="btn danger" disabled={busy !== null || !previewed} onClick={() => setConfirm(true)} title={previewed ? undefined : "Run a preview first"}>
                  <Trash2 size={14} /> Run garbage collection
                </button>
              </div>
            </li>
          </ol>
          {report && (
            <div style={{ marginTop: 20 }}>
              <Callout tone={report.failures.length ? "warn" : "ok"} icon={<CheckCircle2 size={15} />}>
                <strong>{report.dry_run ? "Preview" : "Collection finished"}:</strong> {report.blobs} blobs and {report.manifests} manifests ·{" "}
                {formatBytes(report.reclaimed_bytes)} {report.dry_run ? "can be reclaimed" : "reclaimed"}
                {report.failures.length > 0 && ` · ${report.failures.length} failures`}
              </Callout>
              {report.failures.length > 0 && <pre className="code-block" style={{ marginTop: 12 }}>{report.failures.join("\n")}</pre>}
            </div>
          )}
        </div>
      </Panel>
      {confirm && (
        <ConfirmDialog
          title="Delete unreferenced content?"
          text={`About ${formatBytes(report?.reclaimed_bytes ?? 0)} will be permanently removed from object storage. This cannot be undone.`}
          confirmLabel="Delete permanently"
          confirmValue="collect"
          danger
          busy={busy === "run"}
          onClose={() => setConfirm(false)}
          onConfirm={() => run(false)}
        />
      )}
    </>
  );
}

/* ---------- uploads ---------- */

export function UploadsPage() {
  const { user } = useSession();
  const { data, error, reload } = useResource<{ uploads: UploadSession[] }>(user.is_admin ? "/api/v1/uploads" : null);
  if (!user.is_admin) return <NoAccess />;
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton />;
  const uploads = data.uploads;
  return (
    <>
      <PageHeader
        title="Uploads"
        description="Chunked blob upload sessions in progress. Useful when a push hangs or fails half-way."
        actions={
          <button className="btn" onClick={reload}>
            Refresh
          </button>
        }
      />
      <section className="panel">
        {uploads.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Repository</th>
                  <th>Session</th>
                  <th className="num">Received</th>
                  <th className="num">Expires</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {uploads.map((upload) => (
                  <tr key={upload.id}>
                    <td>
                      <RepositoryName name={upload.repository} />
                    </td>
                    <td>
                      <span className="mono muted">{upload.id.slice(0, 12)}</span> <CopyButton value={upload.id} title="Copy session ID" />
                    </td>
                    <td className="num">{formatBytes(upload.offset)}</td>
                    <td className="num muted">
                      <RelativeTime value={upload.expires_at} />
                    </td>
                    <td>
                      <StatusBadge
                        dot
                        state={upload.status === "Active" ? "info" : upload.status === "Failed" || upload.status === "Aborted" ? "fail" : "ok"}
                        label={upload.status}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState icon={<Upload size={18} />} title="No uploads in progress" text="Sessions appear here while a docker push is uploading layers." />
        )}
      </section>
    </>
  );
}

/* ---------- webhooks ---------- */

export function WebhooksPage() {
  const { user } = useSession();
  const admin = user.is_admin;
  const { data, error, reload } = useResource<{ webhooks: Webhook[]; managed_webhook_id?: string }>("/api/v1/webhooks");
  const [adding, setAdding] = useState(false);
  const [secret, setSecret] = useState<{ url: string; secret: string } | null>(null);
  const [disabling, setDisabling] = useState<Webhook | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton />;
  const managedId = data.managed_webhook_id ?? "";
  const webhooks = data.webhooks;
  return (
    <>
      <PageHeader
        title="Webhooks"
        description="Signed HTTP callbacks when a tag changes. Deploy the immutable digest from the payload, not the tag."
        actions={
          admin && (
            <button className="btn primary" onClick={() => setAdding(true)}>
              <Plus size={14} /> Add endpoint
            </button>
          )
        }
      />
      <section className="panel">
        {webhooks.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Endpoint</th>
                  <th className="hide-sm">Events</th>
                  <th>Status</th>
                  <th className="num hide-sm">Created</th>
                  <th className="shrink" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {webhooks.map((webhook) => (
                  <tr key={webhook.id}>
                    <td>
                      <div className="cell-stack">
                        <span className="mono" style={{ maxWidth: 420, overflow: "hidden", textOverflow: "ellipsis" }} title={webhook.url}>
                          {webhook.url}
                        </span>
                        {webhook.id === managedId && (
                          <small>
                            <Link to="/deployments/cloud" className="inline-link">Managed by Knotree Cloud</Link>
                          </small>
                        )}
                      </div>
                    </td>
                    <td className="hide-sm">
                      {(webhook.events.length ? webhook.events : ["all events"]).map((event) => (
                        <span key={event} className="tag-pill" style={{ marginRight: 4 }}>{event}</span>
                      ))}
                    </td>
                    <td>
                      <StatusBadge dot state={webhook.enabled ? "ok" : "neutral"} label={webhook.enabled ? "Active" : "Disabled"} />
                    </td>
                    <td className="num muted hide-sm">
                      <RelativeTime value={webhook.created_at} />
                    </td>
                    <td>
                      {admin && webhook.enabled && webhook.id !== managedId && (
                        <button className="btn sm danger" onClick={() => setDisabling(webhook)}>
                          Disable
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState
            icon={<WebhookIcon size={18} />}
            title="No webhooks"
            text="Add an endpoint to receive signed tag_updated events whenever an image is published."
            action={
              admin && (
                <button className="btn primary" onClick={() => setAdding(true)}>
                  <Plus size={14} /> Add endpoint
                </button>
              )
            }
          />
        )}
      </section>
      {adding && (
        <AddWebhookModal
          onClose={() => setAdding(false)}
          onCreated={(created) => {
            setAdding(false);
            setSecret(created);
            reload();
          }}
        />
      )}
      {secret && (
        <Modal
          title="Webhook added"
          description="Copy the signing secret now — it isn't shown again."
          onClose={() => setSecret(null)}
          footer={
            <button className="btn primary" onClick={() => setSecret(null)}>
              I've stored it safely
            </button>
          }
        >
          <dl className="kv compact">
            <dt>Endpoint</dt>
            <dd className="mono">{secret.url}</dd>
          </dl>
          <div className="secret">
            <Lock size={14} className="muted" />
            <code>{secret.secret}</code>
            <CopyButton className="btn sm" value={secret.secret} label="Copy" />
          </div>
          <p className="muted" style={{ fontSize: 12.5 }}>Verify the HMAC signature header on every delivery with this secret.</p>
        </Modal>
      )}
      {disabling && (
        <ConfirmDialog
          title="Disable this webhook?"
          text={<>Deliveries to <span className="mono">{disabling.url}</span> stop immediately.</>}
          confirmLabel="Disable webhook"
          danger
          busy={busy}
          onClose={() => setDisabling(null)}
          onConfirm={async () => {
            setBusy(true);
            try {
              await api(`/api/v1/webhooks/${disabling.id}/disable`, { method: "POST" });
              toast("Webhook disabled");
              setDisabling(null);
              reload();
            } catch (reason) {
              toast(friendlyError(reason), "error");
            } finally {
              setBusy(false);
            }
          }}
        />
      )}
    </>
  );
}

function AddWebhookModal({ onClose, onCreated }: { onClose: () => void; onCreated: (value: { url: string; secret: string }) => void }) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await api<{ webhook: Webhook; secret: string }>("/api/v1/webhooks", {
        method: "POST",
        body: JSON.stringify({ url, events: ["tag_updated"] })
      });
      onCreated({ url: result.webhook.url, secret: result.secret });
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title="Add webhook endpoint"
      onClose={onClose}
      footer={
        <>
          <button className="btn ghost" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" form="add-webhook" disabled={busy || !url}>
            {busy ? "Adding…" : "Add endpoint"}
          </button>
        </>
      }
    >
      <form id="add-webhook" className="form-grid" onSubmit={submit}>
        <label className="field">
          <span>Endpoint URL</span>
          <input type="url" required autoFocus placeholder="https://hooks.example.com/registry" value={url} onChange={(event) => setUrl(event.target.value)} className="mono" />
          <small>Must be HTTPS and reachable from the registry.</small>
        </label>
        <div className="field">
          <span>Events</span>
          <div className="row">
            <span className="tag-pill">tag_updated</span>
            <small className="muted">Sent whenever a tag points at a new digest.</small>
          </div>
        </div>
        {error && <Callout tone="error">{error}</Callout>}
      </form>
    </Modal>
  );
}

/* ---------- account & settings ---------- */

/** Passwords and two-step verification live in the Knotree account. */
export function SecuritySettingsPage() {
  const { user, host } = useSession();
  return (
    <>
      <PageHeader title="Account & security" description="Registry signs you in with your Knotree account. Password, email and two-step verification are managed there." />
      <div className="grid">
        <Panel className="span-7" title="Your account">
          <div className="panel-body">
            <div className="row" style={{ gap: 14, marginBottom: 18, flexWrap: "nowrap" }}>
              <span className="avatar" style={{ width: 44, height: 44, flexBasis: 44, fontSize: 15 }}>{user.username.slice(0, 2).toUpperCase()}</span>
              <div>
                <div style={{ fontWeight: 600, fontSize: 15 }}>{user.username}</div>
                <div className="muted" style={{ fontSize: 13 }}>{user.is_admin ? "Administrator of this registry" : "Member"}</div>
              </div>
            </div>
            <dl className="kv compact">
              <dt>Sign-in</dt>
              <dd>Knotree Accounts (single sign-on)</dd>
              <dt>Role</dt>
              <dd>{user.is_admin ? <StatusBadge state="accent" label="Administrator" /> : <StatusBadge state="neutral" label="Member" />}</dd>
              <dt>User ID</dt>
              <dd className="mono muted" style={{ fontSize: 12 }}>{user.id}</dd>
            </dl>
          </div>
          <div className="panel-foot">
            <span>Password, email and 2-step verification</span>
            <a className="btn sm" href="https://accounts.knotree.com/account/security" target="_blank" rel="noreferrer">
              Open Knotree Accounts <ExternalLink size={12} />
            </a>
          </div>
        </Panel>
        <Panel className="span-5" title="Docker & CI" description="Your Knotree password never works with docker login.">
          <div className="panel-body stack-lg">
            <Callout tone="info" icon={<ShieldCheck size={15} />}>
              Create a scoped access token and use it as the password for <code>{host}</code>.
            </Callout>
            <Link className="btn full" to="/security/tokens?create=1">
              <Plus size={14} /> New access token
            </Link>
          </div>
        </Panel>
      </div>
    </>
  );
}

export function SettingsPage() {
  const { instance } = useSession();
  if (!instance) return <Skeleton rows={6} />;
  const groups: { title: string; description: string; rows: [string, React.ReactNode][] }[] = [
    {
      title: "Instance",
      description: "Identity of this registry.",
      rows: [
        ["Token service", <span className="mono">{instance.token_service}</span>],
        ["Environment", <StatusBadge state="neutral" label={instance.environment} />],
        ["Public URL", <span className="row" style={{ gap: 4 }}><span className="mono">{instance.public_url}</span><CopyButton value={instance.public_url} /></span>],
        ["Registry host", <span className="row" style={{ gap: 4 }}><span className="mono">{instance.registry_host}</span><CopyButton value={instance.registry_host} /></span>]
      ]
    },
    {
      title: "Access",
      description: "How people and machines get in.",
      rows: [
        ["Registration", instance.registration === "closed" ? "Closed — only administrators add accounts" : instance.registration],
        ["Web sign-in", "Knotree Accounts (SSO)"],
        ["Registry token lifetime", `${formatDuration(instance.token_ttl_seconds)} (${instance.token_ttl_seconds}s)`]
      ]
    },
    {
      title: "Storage",
      description: "Where image content lives.",
      rows: [
        ["Backend", instance.storage_backend === "r2" ? "Cloudflare R2" : instance.storage_backend],
        ["Bucket", <span className="mono">{instance.storage_bucket ?? "—"}</span>],
        ["Blob serving", <span className="mono">{instance.pull_mode}</span>]
      ]
    }
  ];
  return (
    <>
      <PageHeader title="Settings" description="Read-only. These values come from the server's environment and change on redeploy." />
      <div className="stack-lg">
        {groups.map((group) => (
          <Panel key={group.title} title={group.title} description={group.description}>
            <div className="panel-body" style={{ paddingTop: 4, paddingBottom: 4 }}>
              <dl className="kv">
                {group.rows.map(([label, value]) => (
                  <Row key={label} label={label} value={value} />
                ))}
              </dl>
            </div>
          </Panel>
        ))}
      </div>
    </>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </>
  );
}

export function MembersPage() {
  const { user } = useSession();
  const { data } = useResource<{ tokens: Token[] }>("/api/v1/auth/tokens");
  const active = (data?.tokens ?? []).filter((token) => !token.revoked_at).length;
  return (
    <>
      <PageHeader title="Members" description="People who can sign in to this registry." />
      <section className="panel">
        <div className="table-wrap">
          <table className="data">
            <thead>
              <tr>
                <th>Member</th>
                <th>Role</th>
                <th className="num">Access tokens</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <span className="row" style={{ gap: 10, flexWrap: "nowrap" }}>
                    <span className="avatar">{user.username.slice(0, 2).toUpperCase()}</span>
                    <span className="cell-stack">
                      <span style={{ fontWeight: 500 }}>{user.username}</span>
                      <small>You</small>
                    </span>
                  </span>
                </td>
                <td>{user.is_admin ? <StatusBadge state="accent" label="Administrator" /> : <StatusBadge state="neutral" label="Member" />}</td>
                <td className="num">
                  <Link className="inline-link" to="/security/tokens">{active} active</Link>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div className="panel-foot">
          <span className="row" style={{ gap: 8 }}>
            <Users size={14} /> Invitations and roles aren't enabled on this instance yet.
          </span>
          <a className="btn sm ghost" href="https://accounts.knotree.com" target="_blank" rel="noreferrer">
            Knotree Accounts <ArrowUpRight size={12} />
          </a>
        </div>
      </section>
    </>
  );
}
