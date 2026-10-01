import { FormEvent, useEffect, useMemo, useState } from "react";
import { QRCodeCanvas } from "qrcode.react";
import { Link, useSearchParams } from "react-router-dom";
import {
  AuditEvent,
  GcReport,
  Instance,
  StorageOverview,
  Token,
  TotpSetup,
  TotpStatus,
  UploadSession,
  User,
  Webhook,
  api,
  friendlyError
} from "./api";
import { eventLabel, eventResult, formatBytes } from "./format";
import {
  ConfirmDialog,
  CopyButton,
  DigestView,
  Drawer,
  EmptyState,
  ErrorState,
  HealthDot,
  Metric,
  Modal,
  PageHeader,
  RelativeTime,
  RepositoryName,
  Skeleton,
  StatusBadge,
  useToast
} from "./ui";

export function AuditPage() {
  const [events, setEvents] = useState<AuditEvent[] | null>(null);
  const [error, setError] = useState("");
  const [params, setParams] = useSearchParams();
  const [selected, setSelected] = useState<AuditEvent | null>(null);
  const action = params.get("action") ?? "";
  const actor = params.get("actor") ?? "";
  const repository = params.get("repository") ?? "";
  const result = params.get("result") ?? "";
  function load() {
    api<{ events: AuditEvent[] }>("/api/v1/audit?limit=200")
      .then((payload) => setEvents(payload.events))
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  const filtered = useMemo(() => {
    return (events ?? []).filter((event) => {
      if (action && event.kind !== action) return false;
      if (actor && (event.actor ?? "") !== actor) return false;
      if (repository && (event.repository ?? "") !== repository) return false;
      if (result && eventResult(event.kind) !== result) return false;
      return true;
    });
  }, [events, action, actor, repository, result]);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!events) return <Skeleton />;
  const actions = Array.from(new Set(events.map((event) => event.kind)));
  const actors = Array.from(new Set(events.map((event) => event.actor).filter(Boolean))) as string[];
  const repos = Array.from(new Set(events.map((event) => event.repository).filter(Boolean))) as string[];
  function set(key: string, value: string) {
    if (value) params.set(key, value);
    else params.delete(key);
    setParams(params, { replace: true });
  }
  return (
    <>
      <PageHeader title="Audit log" description="Credential, repository, webhook, and authentication events." />
      <div className="filters">
        <select value={actor} onChange={(event) => set("actor", event.target.value)} aria-label="Actor">
          <option value="">All actors</option>
          {actors.map((value) => (
            <option key={value}>{value}</option>
          ))}
        </select>
        <select value={action} onChange={(event) => set("action", event.target.value)} aria-label="Action">
          <option value="">All actions</option>
          {actions.map((value) => (
            <option key={value} value={value}>
              {eventLabel(value)}
            </option>
          ))}
        </select>
        <select value={repository} onChange={(event) => set("repository", event.target.value)} aria-label="Repository">
          <option value="">All repositories</option>
          {repos.map((value) => (
            <option key={value}>{value}</option>
          ))}
        </select>
        <select value={result} onChange={(event) => set("result", event.target.value)} aria-label="Result">
          <option value="">Any result</option>
          <option value="success">Success</option>
          <option value="failed">Failed</option>
        </select>
      </div>
      <section className="panel">
        {filtered.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Actor</th>
                  <th>Action</th>
                  <th>Resource</th>
                  <th>Result</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((event) => (
                  <tr key={event.id} onClick={() => setSelected(event)}>
                    <td>
                      <RelativeTime value={event.occurred_at} />
                    </td>
                    <td>{event.actor ?? "system"}</td>
                    <td style={{ textTransform: "capitalize" }}>{eventLabel(event.kind)}</td>
                    <td className="mono">{event.repository ?? event.digest ?? "—"}{event.tag ? `:${event.tag}` : ""}</td>
                    <td>
                      <StatusBadge
                        state={event.kind.includes("failed") ? "fail" : "ok"}
                        label={event.kind.includes("failed") ? "Failed" : "Success"}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState title="No events yet" text="Security-sensitive activity will appear here." />
        )}
      </section>
      {selected && (
        <Drawer title={eventLabel(selected.kind)} onClose={() => setSelected(null)}>
          <label className="field">
            <span>Actor</span>
            <strong>{selected.actor ?? "system"}</strong>
          </label>
          <label className="field">
            <span>Resource</span>
            <code>{selected.repository ?? "—"}</code>
          </label>
          {selected.digest && (
            <label className="field">
              <span>Digest</span>
              <DigestView value={selected.digest} />
            </label>
          )}
          <label className="field">
            <span>Time</span>
            <RelativeTime value={selected.occurred_at} />
          </label>
          <pre className="secret-box">{JSON.stringify(selected.metadata ?? {}, null, 2)}</pre>
        </Drawer>
      )}
    </>
  );
}

export function StoragePage() {
  const [data, setData] = useState<StorageOverview | null>(null);
  const [error, setError] = useState("");
  const [instance, setInstance] = useState<Instance | null>(null);
  function load() {
    Promise.all([api<StorageOverview>("/api/v1/storage"), api<Instance>("/api/v1/instance")])
      .then(([storage, info]) => {
        setData(storage);
        setInstance(info);
      })
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!data) return <Skeleton />;
  return (
    <>
      <PageHeader title="Storage" description="Find repositories that occupy the most object storage." />
      <div className="metrics">
        <Metric label="Total stored" value={formatBytes(data.total_bytes)} />
        <Metric label="Referenced" value={formatBytes(data.referenced_bytes)} />
        <Metric label="Unreferenced" value={formatBytes(data.unreferenced_bytes)} />
        <Metric label="Repositories" value={String(data.repository_count)} />
      </div>
      {instance && (
        <section className="panel panel-pad" style={{ marginBottom: 16 }}>
          <div className="meta-row">
            <div>
              <span>Storage provider</span>
              <strong>{instance.storage_backend === "r2" ? "Cloudflare R2" : instance.storage_backend}</strong>
            </div>
            <div>
              <span>Bucket</span>
              <strong>{instance.storage_bucket ?? "local / memory"}</strong>
            </div>
            <div>
              <span>Connection</span>
              <StatusBadge state="ok" label="Configured" />
            </div>
          </div>
        </section>
      )}
      <section className="panel">
        <div className="table-wrap">
          <table className="data">
            <thead>
              <tr>
                <th>Repository</th>
                <th className="num">Storage</th>
                <th className="num">Manifests</th>
                <th className="num">Tags</th>
                <th>Last push</th>
              </tr>
            </thead>
            <tbody>
              {[...data.repositories]
                .sort((left, right) => (right.size ?? 0) - (left.size ?? 0))
                .map((repo) => (
                  <tr key={repo.name}>
                    <td>
                      <RepositoryName name={repo.name} />
                    </td>
                    <td className="num">{formatBytes(repo.size ?? 0)}</td>
                    <td className="num">{repo.manifest_count ?? 0}</td>
                    <td className="num">{repo.tag_count ?? 0}</td>
                    <td>
                      <RelativeTime value={repo.updated_at} />
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  );
}

export function GarbageCollectionPage({ admin }: { admin: boolean }) {
  const [storage, setStorage] = useState<StorageOverview | null>(null);
  const [preview, setPreview] = useState<GcReport | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<"dry" | "run" | null>(null);
  const [confirm, setConfirm] = useState(false);
  const toast = useToast();
  function load() {
    api<StorageOverview>("/api/v1/storage")
      .then(setStorage)
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  if (!admin) return <ErrorState message="You don't have permission to access this page." />;
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!storage) return <Skeleton />;
  async function run(dry_run: boolean) {
    setBusy(dry_run ? "dry" : "run");
    try {
      const report = await api<GcReport>("/api/v1/admin/gc", { method: "POST", body: JSON.stringify({ dry_run }) });
      setPreview(report);
      toast(dry_run ? "Dry run complete" : "Garbage collection finished");
      load();
    } catch (reason) {
      toast(friendlyError(reason), "error");
    } finally {
      setBusy(null);
      setConfirm(false);
    }
  }
  return (
    <>
      <PageHeader
        title="Garbage collection"
        description="Garbage collection removes blobs and manifests that are no longer referenced."
        actions={
          <>
            <button className="btn" disabled={busy !== null} onClick={() => run(true)}>
              {busy === "dry" ? "Running…" : "Dry run"}
            </button>
            <button className="btn danger" disabled={busy !== null} onClick={() => setConfirm(true)}>
              Run garbage collection
            </button>
          </>
        }
      />
      <div className="metrics">
        <Metric label="Unreferenced content" value={formatBytes(storage.unreferenced_bytes)} />
        <Metric label="Eligible for cleanup" value={preview ? formatBytes(preview.reclaimed_bytes) : "Run a dry run"} />
      </div>
      {preview && (
        <section className="panel panel-pad">
          <h2>{preview.dry_run ? "Dry run preview" : "Last run"}</h2>
          <p>
            {preview.blobs} blobs · {preview.manifests} manifests · {formatBytes(preview.reclaimed_bytes)} recoverable
          </p>
          {preview.failures.length > 0 && <pre className="secret-box">{preview.failures.join("\n")}</pre>}
        </section>
      )}
      {confirm && (
        <ConfirmDialog
          title="Run garbage collection?"
          text="Unreferenced blobs and manifests past the grace period will be deleted from object storage."
          confirmLabel="Run garbage collection"
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

export function UploadsPage({ admin }: { admin: boolean }) {
  const [uploads, setUploads] = useState<UploadSession[] | null>(null);
  const [error, setError] = useState("");
  function load() {
    api<{ uploads: UploadSession[] }>("/api/v1/uploads")
      .then((result) => setUploads(result.uploads))
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  if (!admin) return <ErrorState message="You don't have permission to access this page." />;
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!uploads) return <Skeleton />;
  return (
    <>
      <PageHeader title="Uploads" description="Active and recent blob upload sessions. This view is for debugging." />
      <section className="panel">
        {uploads.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Repository</th>
                  <th>Upload ID</th>
                  <th className="num">Received</th>
                  <th>Expires</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {uploads.map((upload) => (
                  <tr key={upload.id}>
                    <td>
                      <RepositoryName name={upload.repository} />
                    </td>
                    <td className="mono">{upload.id.slice(0, 8)}…</td>
                    <td className="num">{formatBytes(upload.offset)}</td>
                    <td>
                      <RelativeTime value={upload.expires_at} />
                    </td>
                    <td>
                      <StatusBadge
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
          <EmptyState title="No upload sessions" text="Chunked blob uploads will appear here while they are in progress." />
        )}
      </section>
    </>
  );
}

export function WebhooksPage({ admin }: { admin: boolean }) {
  const [webhooks, setWebhooks] = useState<Webhook[] | null>(null);
  const [managedId, setManagedId] = useState("");
  const [error, setError] = useState("");
  const [url, setUrl] = useState("");
  const [secret, setSecret] = useState("");
  const toast = useToast();
  function load() {
    api<{ webhooks: Webhook[]; managed_webhook_id?: string }>("/api/v1/webhooks")
      .then((result) => {
        setWebhooks(result.webhooks);
        setManagedId(result.managed_webhook_id ?? "");
      })
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  async function create(event: FormEvent) {
    event.preventDefault();
    try {
      const result = await api<{ webhook: Webhook; secret: string }>("/api/v1/webhooks", {
        method: "POST",
        body: JSON.stringify({ url, events: ["tag_updated"] })
      });
      setSecret(result.secret);
      setUrl("");
      toast("Webhook created");
      load();
    } catch (reason) {
      toast(friendlyError(reason), "error");
    }
  }
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!webhooks) return <Skeleton />;
  return (
    <>
      <PageHeader title="Webhooks" description="Deliver signed image tag events to a deployment endpoint; consumers should deploy the immutable digest in the payload." />
      {secret && (
        <div className="alert warn" style={{ marginBottom: 16 }}>
          Save this webhook secret now. It is not returned by later requests.
          <div className="secret-box" style={{ marginTop: 8 }}>
            {secret}
          </div>
          <CopyButton value={secret} label="Copy secret" />
        </div>
      )}
      {admin && (
        <section className="panel panel-pad" style={{ marginBottom: 16 }}>
          <form className="filters" onSubmit={create}>
            <input type="url" required placeholder="https://hooks.example.com/registry" value={url} onChange={(event) => setUrl(event.target.value)} />
            <button className="btn primary">Add webhook</button>
          </form>
        </section>
      )}
      <section className="panel">
        {webhooks.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>URL</th>
                  <th>Events</th>
                  <th>Status</th>
                  <th>Created</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {webhooks.map((webhook) => (
                  <tr key={webhook.id}>
                    <td className="mono">
                      {webhook.url}
                      {webhook.id === managedId && (
                        <Link to="/deployments/cloud" style={{ marginLeft: 8 }}>
                          <StatusBadge state="info" label="Managed · Knotree Cloud" />
                        </Link>
                      )}
                    </td>
                    <td>{webhook.events.join(", ") || "all events"}</td>
                    <td>
                      <StatusBadge state={webhook.enabled ? "ok" : "fail"} label={webhook.enabled ? "Active" : "Disabled"} />
                    </td>
                    <td>
                      <RelativeTime value={webhook.created_at} />
                    </td>
                    <td>
                      {admin && webhook.enabled && webhook.id !== managedId && (
                        <button
                          className="btn sm danger"
                          onClick={async () => {
                            if (!window.confirm("Disable this webhook?")) return;
                            await api(`/api/v1/webhooks/${webhook.id}/disable`, { method: "POST" });
                            toast("Webhook disabled");
                            load();
                          }}
                        >
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
          <EmptyState title="No webhooks" text="Add an endpoint to receive signed image publication events." />
        )}
      </section>
    </>
  );
}

export function SecuritySettingsPage({ user }: { user: User }) {
  const toast = useToast();
  const [status, setStatus] = useState<TotpStatus | null>(null);
  const [loadError, setLoadError] = useState("");
  const [passwords, setPasswords] = useState({ current: "", next: "", confirm: "" });
  const [passwordBusy, setPasswordBusy] = useState(false);
  const [passwordError, setPasswordError] = useState("");
  const [setupOpen, setSetupOpen] = useState(false);
  const [setup, setSetup] = useState<TotpSetup | null>(null);
  const [setupPassword, setSetupPassword] = useState("");
  const [setupCode, setSetupCode] = useState("");
  const [setupBusy, setSetupBusy] = useState(false);
  const [setupError, setSetupError] = useState("");
  const [disableOpen, setDisableOpen] = useState(false);
  const [disablePassword, setDisablePassword] = useState("");
  const [disableCode, setDisableCode] = useState("");
  const [disableBusy, setDisableBusy] = useState(false);
  const [disableError, setDisableError] = useState("");

  function loadStatus() {
    setLoadError("");
    api<TotpStatus>("/api/v1/auth/2fa")
      .then(setStatus)
      .catch((reason) => setLoadError(friendlyError(reason)));
  }
  useEffect(loadStatus, []);

  async function changePassword(event: FormEvent) {
    event.preventDefault();
    setPasswordBusy(true);
    setPasswordError("");
    try {
      await api("/api/v1/auth/password", {
        method: "POST",
        body: JSON.stringify({
          current_password: passwords.current,
          new_password: passwords.next,
          confirm_password: passwords.confirm
        })
      });
      setPasswords({ current: "", next: "", confirm: "" });
      toast("Password changed. Other sessions were signed out.");
    } catch (reason) {
      setPasswordError(friendlyError(reason));
    } finally {
      setPasswordBusy(false);
    }
  }

  function closeSetup() {
    setSetupOpen(false);
    setSetup(null);
    setSetupPassword("");
    setSetupCode("");
    setSetupError("");
  }

  async function startSetup(event: FormEvent) {
    event.preventDefault();
    setSetupBusy(true);
    setSetupError("");
    try {
      const result = await api<TotpSetup>("/api/v1/auth/2fa/setup", {
        method: "POST",
        body: JSON.stringify({ password: setupPassword })
      });
      setSetup(result);
    } catch (reason) {
      setSetupError(friendlyError(reason));
    } finally {
      setSetupBusy(false);
    }
  }

  async function confirmSetup(event: FormEvent) {
    event.preventDefault();
    setSetupBusy(true);
    setSetupError("");
    try {
      await api<TotpStatus>("/api/v1/auth/2fa/confirm", {
        method: "POST",
        body: JSON.stringify({ code: setupCode })
      });
      setStatus({ enabled: true });
      closeSetup();
      toast("Two-factor authentication enabled");
    } catch (reason) {
      setSetupError(friendlyError(reason));
    } finally {
      setSetupBusy(false);
    }
  }

  async function disableTwoFactor(event: FormEvent) {
    event.preventDefault();
    setDisableBusy(true);
    setDisableError("");
    try {
      await api<TotpStatus>("/api/v1/auth/2fa/disable", {
        method: "POST",
        body: JSON.stringify({ password: disablePassword, code: disableCode })
      });
      setStatus({ enabled: false });
      setDisableOpen(false);
      setDisablePassword("");
      setDisableCode("");
      toast("Two-factor authentication disabled");
    } catch (reason) {
      setDisableError(friendlyError(reason));
    } finally {
      setDisableBusy(false);
    }
  }

  return (
    <>
      <PageHeader title="Account security" description="Password, sessions, and two-factor authentication." />
      <div className="grid-2">
        <section className="panel panel-pad grid-gap">
          <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
            <span className="avatar">{user.username.slice(0, 1).toUpperCase()}</span>
            <div>
              <strong>{user.username}</strong>
              <div style={{ color: "var(--muted)" }}>{user.is_admin ? "Administrator" : "Member"}</div>
            </div>
          </div>
          <div>
            <strong>Password</strong>
            <div style={{ color: "var(--muted)", fontSize: 13, marginTop: 4 }}>Changing it signs out all other browser sessions.</div>
          </div>
          <form className="stack" onSubmit={changePassword}>
            <label className="field">
              <span>Current password</span>
              <input type="password" autoComplete="current-password" value={passwords.current} onChange={(event) => setPasswords({ ...passwords, current: event.target.value })} required />
            </label>
            <label className="field">
              <span>New password</span>
              <input type="password" autoComplete="new-password" minLength={12} value={passwords.next} onChange={(event) => setPasswords({ ...passwords, next: event.target.value })} required />
            </label>
            <label className="field">
              <span>Confirm new password</span>
              <input type="password" autoComplete="new-password" minLength={12} value={passwords.confirm} onChange={(event) => setPasswords({ ...passwords, confirm: event.target.value })} required />
            </label>
            {passwordError && <div className="alert error" role="alert">{passwordError}</div>}
            <button className="btn primary" disabled={passwordBusy}>
              {passwordBusy ? "Changing…" : "Change password"}
            </button>
          </form>
          <div className="health-row">
            <div>
              <strong>Two-factor authentication</strong>
              <div style={{ color: "var(--muted)", fontSize: 13 }}>Use an authenticator app for an extra sign-in challenge.</div>
            </div>
            {status?.enabled ? <StatusBadge state="ok" label="Enabled" /> : <StatusBadge state="neutral" label="Not configured" />}
          </div>
          {loadError && <div className="alert error" role="alert">{loadError}</div>}
          {status?.enabled ? (
            <button className="btn danger" onClick={() => { setDisableError(""); setDisableOpen(true); }}>
              Disable two-factor authentication
            </button>
          ) : (
            <button className="btn" onClick={() => { setSetupError(""); setSetupOpen(true); }}>
              Set up authenticator app
            </button>
          )}
        </section>
        <section className="panel panel-pad grid-gap">
          <h2>Active sessions</h2>
          <div className="health-row">
            <HealthDot status="ok" />
            <div>
              <strong>This browser</strong>
              <div style={{ color: "var(--muted)", fontSize: 13 }}>Current session · last active now</div>
            </div>
          </div>
          <p style={{ color: "var(--muted)", fontSize: 13 }}>Remote session listing is not exposed by this control plane. Sign out to invalidate this cookie.</p>
        </section>
      </div>
      {setupOpen && (
        <Modal
          title={setup ? "Confirm authenticator setup" : "Set up authenticator app"}
          onClose={closeSetup}
          wide
          footer={
            <>
              <button className="btn ghost" onClick={closeSetup}>Cancel</button>
              {!setup ? (
                <button className="btn primary" form="totp-start" disabled={setupBusy}>{setupBusy ? "Preparing…" : "Continue"}</button>
              ) : (
                <button className="btn primary" form="totp-confirm" disabled={setupBusy || setupCode.length !== 6}>{setupBusy ? "Verifying…" : "Enable 2FA"}</button>
              )}
            </>
          }
        >
          {!setup ? (
            <form id="totp-start" className="stack" onSubmit={startSetup}>
              <p>Confirm your current password to generate a one-time setup secret.</p>
              <label className="field">
                <span>Current password</span>
                <input autoFocus type="password" autoComplete="current-password" value={setupPassword} onChange={(event) => setSetupPassword(event.target.value)} required />
              </label>
              {setupError && <div className="alert error" role="alert">{setupError}</div>}
            </form>
          ) : (
            <form id="totp-confirm" className="stack" onSubmit={confirmSetup}>
              <div className="totp-setup">
                <div className="qr-panel"><QRCodeCanvas value={setup.otpauth_uri} size={184} includeMargin /></div>
                <div className="stack">
                  <p>Scan this QR code in Google Authenticator, 1Password, Authy or another TOTP app.</p>
                  <div className="secret-box mono">{setup.secret}</div>
                  <CopyButton value={setup.secret} label="Copy secret" />
                  <details className="details">
                    <summary>Show setup URI</summary>
                    <div className="secret-box mono">{setup.otpauth_uri}</div>
                    <CopyButton value={setup.otpauth_uri} label="Copy URI" />
                  </details>
                </div>
              </div>
              <label className="field">
                <span>Authenticator code</span>
                <input autoFocus inputMode="numeric" autoComplete="one-time-code" maxLength={6} pattern="[0-9]{6}" value={setupCode} onChange={(event) => setSetupCode(event.target.value.replace(/\D/g, "").slice(0, 6))} placeholder="000000" required />
                <small>Enter the current 6-digit code to finish setup.</small>
              </label>
              {setupError && <div className="alert error" role="alert">{setupError}</div>}
            </form>
          )}
        </Modal>
      )}
      {disableOpen && (
        <Modal title="Disable two-factor authentication" onClose={() => !disableBusy && setDisableOpen(false)} footer={<><button className="btn ghost" onClick={() => setDisableOpen(false)}>Cancel</button><button className="btn danger solid" form="totp-disable" disabled={disableBusy || disableCode.length !== 6}>{disableBusy ? "Disabling…" : "Disable 2FA"}</button></>}>
          <form id="totp-disable" className="stack" onSubmit={disableTwoFactor}>
            <div className="alert warn">You will only need your password to sign in after this change.</div>
            <label className="field"><span>Current password</span><input autoFocus type="password" autoComplete="current-password" value={disablePassword} onChange={(event) => setDisablePassword(event.target.value)} required /></label>
            <label className="field"><span>Authenticator code</span><input inputMode="numeric" autoComplete="one-time-code" maxLength={6} pattern="[0-9]{6}" value={disableCode} onChange={(event) => setDisableCode(event.target.value.replace(/\D/g, "").slice(0, 6))} required /></label>
            {disableError && <div className="alert error" role="alert">{disableError}</div>}
          </form>
        </Modal>
      )}
    </>
  );
}

export function SettingsPage({ instance }: { instance: Instance | null }) {
  if (!instance) return <Skeleton />;
  return (
    <>
      <PageHeader title="Settings" description="Instance identity, registry domain, and retention defaults." />
      <section className="panel panel-pad grid-gap">
        <label className="field">
          <span>Instance name</span>
          <input value={instance.token_service} readOnly />
        </label>
        <label className="field">
          <span>Registry domain</span>
          <input value={instance.registry_host} readOnly />
        </label>
        <label className="field">
          <span>Environment</span>
          <input value={instance.environment} readOnly />
        </label>
        <label className="field">
          <span>Registration</span>
          <select value={instance.registration} disabled>
            <option value="closed">Closed — only administrators can create accounts</option>
            <option value="invite">Invite only</option>
            <option value="open">Open</option>
          </select>
        </label>
        <label className="field">
          <span>Blob serving mode</span>
          <input value={instance.pull_mode} readOnly />
        </label>
        <label className="field">
          <span>Token lifetime</span>
          <input value={`${instance.token_ttl_seconds} seconds`} readOnly />
        </label>
      </section>
    </>
  );
}

export function PlaceholderPage({
  title,
  text
}: {
  title: string;
  text: string;
}) {
  return (
    <>
      <PageHeader title={title} description={text} />
      <section className="panel">
        <EmptyState title={`No ${title.toLowerCase()} yet`} text={text} />
      </section>
    </>
  );
}

export function MembersPage() {
  const [tokens, setTokens] = useState<Token[] | null>(null);
  useEffect(() => {
    api<{ tokens: Token[] }>("/api/v1/auth/tokens").then((result) => setTokens(result.tokens)).catch(() => setTokens([]));
  }, []);
  return (
    <>
      <PageHeader title="Members" description="Namespace membership is managed with account roles on this instance." />
      <section className="panel">
        <EmptyState
          title="No additional members"
          text="This control plane currently authenticates a bootstrap administrator and scoped access tokens. Invite and role APIs are not enabled."
        />
      </section>
      {tokens && tokens.length > 0 && (
        <p style={{ marginTop: 12, color: "var(--muted)" }}>{tokens.length} access tokens can act on repositories.</p>
      )}
    </>
  );
}
