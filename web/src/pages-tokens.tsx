import { FormEvent, useMemo, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { AlertTriangle, ArrowLeft, KeyRound, Lock, Plus } from "lucide-react";
import { Token, api, friendlyError, useResource } from "./api";
import { formatAbsolute } from "./format";
import { useSession } from "./session";
import {
  Callout,
  CommandBox,
  ConfirmDialog,
  CopyButton,
  EmptyState,
  ErrorState,
  Modal,
  PageHeader,
  Panel,
  RelativeTime,
  SearchInput,
  Segmented,
  Skeleton,
  StatusBadge,
  Tone,
  permissionHelp,
  useToast
} from "./ui";

type TokenState = { tone: Tone; label: string };

function tokenState(token: Token): TokenState {
  if (token.revoked_at) return { tone: "neutral", label: "Revoked" };
  const now = Date.now() / 1000;
  if (token.expires_at && token.expires_at < now) return { tone: "fail", label: "Expired" };
  if (token.expires_at && token.expires_at - now < 7 * 86400) return { tone: "warn", label: "Expiring soon" };
  return { tone: "ok", label: "Active" };
}

function Scopes({ token }: { token: Token }) {
  return (
    <>
      {token.namespace_pull && (
        <span className="scope">
          <span className="repo">{token.namespace_pull}/*</span>
          <span className="acts">pull</span>
        </span>
      )}
      {token.scopes.map((scope) => (
        <span className="scope" key={scope.repository}>
          <span className="repo">{scope.repository}</span>
          <span className="acts">{scope.actions.join(" · ")}</span>
        </span>
      ))}
    </>
  );
}

export function TokensPage() {
  const { host, user } = useSession();
  const { data, error, reload } = useResource<{ tokens: Token[] }>("/api/v1/auth/tokens");
  const [secret, setSecret] = useState<{ token: Token; secret: string } | null>(null);
  const [params, setParams] = useSearchParams();
  const create = params.get("create") === "1";
  const navigate = useNavigate();
  const query = params.get("q") ?? "";
  const status = (params.get("status") as "active" | "all" | "revoked") ?? "active";
  const tokens = data?.tokens;

  function setParam(key: string, value: string) {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    setParams(next, { replace: true });
  }

  const counts = useMemo(
    () => ({
      active: (tokens ?? []).filter((token) => !token.revoked_at).length,
      revoked: (tokens ?? []).filter((token) => token.revoked_at).length
    }),
    [tokens]
  );
  const filtered = useMemo(() => {
    const q = query.toLowerCase();
    return (tokens ?? []).filter(
      (token) =>
        (status === "all" || (status === "active" ? !token.revoked_at : !!token.revoked_at)) &&
        (!q ||
          token.name.toLowerCase().includes(q) ||
          token.prefix.includes(q) ||
          token.scopes.some((scope) => scope.repository.includes(q)))
    );
  }, [tokens, query, status]);

  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!tokens) return <Skeleton />;

  return (
    <>
      <PageHeader
        title="Access tokens"
        description="Scoped credentials for Docker, CI pipelines and servers. Use one as the password for docker login."
        actions={
          <button className="btn primary" onClick={() => setParam("create", "1")}>
            <Plus size={14} /> New token
          </button>
        }
      />
      {tokens.length > 0 && (
        <div className="toolbar">
          <SearchInput value={query} onChange={(value) => setParam("q", value)} placeholder="Filter by name, prefix or repository" />
          <span className="spacer" />
          <Segmented
            label="Status"
            value={status}
            onChange={(value) => setParam("status", value === "active" ? "" : value)}
            options={[
              { id: "active", label: `Active ${counts.active}` },
              { id: "revoked", label: `Revoked ${counts.revoked}` },
              { id: "all", label: "All" }
            ]}
          />
        </div>
      )}
      <section className="panel">
        {filtered.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Token</th>
                  <th>Access</th>
                  <th className="hide-sm">Last used</th>
                  <th className="hide-md">Expires</th>
                  <th>Status</th>
                  <th className="shrink" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {filtered.map((token) => {
                  const state = tokenState(token);
                  return (
                    <tr key={token.id} className="clickable" onClick={() => navigate(`/security/tokens/${token.id}`)}>
                      <td>
                        <div className="cell-stack">
                          <span style={{ fontWeight: 500 }}>{token.name}</span>
                          <small className="mono">{token.prefix}••••••</small>
                        </div>
                      </td>
                      <td style={{ whiteSpace: "normal" }}>
                        <Scopes token={token} />
                      </td>
                      <td className="hide-sm muted">
                        <RelativeTime value={token.last_used_at} />
                      </td>
                      <td className="hide-md muted">{token.expires_at ? <RelativeTime value={token.expires_at} /> : "Never"}</td>
                      <td>
                        <StatusBadge dot state={state.tone} label={state.label} />
                      </td>
                      <td>
                        <div className="row-actions">{!token.revoked_at && <RevokeButton id={token.id} name={token.name} onDone={reload} />}</div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        ) : tokens.length ? (
          <EmptyState icon={<KeyRound size={18} />} title="No matching tokens" text="Try another filter." />
        ) : (
          <EmptyState
            icon={<KeyRound size={18} />}
            title="No access tokens yet"
            text="Create a token to authenticate Docker, a CI pipeline or a deployment server."
            action={
              <button className="btn primary" onClick={() => setParam("create", "1")}>
                <Plus size={14} /> New token
              </button>
            }
          />
        )}
      </section>
      {create && (
        <CreateTokenModal
          onClose={() => setParam("create", "")}
          onCreated={(created) => {
            setParam("create", "");
            setSecret(created);
            reload();
          }}
        />
      )}
      {secret && <TokenSecretModal host={host} username={user.username} secret={secret.secret} name={secret.token.name} onClose={() => setSecret(null)} />}
    </>
  );
}

function RevokeButton({ id, name, onDone, large }: { id: string; name: string; onDone: () => void; large?: boolean }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  return (
    <>
      <button
        className={`btn danger ${large ? "" : "sm"}`}
        onClick={(event) => {
          event.stopPropagation();
          setOpen(true);
        }}
      >
        Revoke
      </button>
      {open && (
        <div onClick={(event) => event.stopPropagation()}>
          <ConfirmDialog
            title={`Revoke “${name}”?`}
            text="Anything using this token stops being able to pull or push immediately. This cannot be undone."
            confirmLabel="Revoke token"
            danger
            busy={busy}
            onClose={() => setOpen(false)}
            onConfirm={async () => {
              setBusy(true);
              try {
                await api(`/api/v1/auth/tokens/${id}/revoke`, { method: "POST" });
                toast("Token revoked");
                setOpen(false);
                onDone();
              } catch (reason) {
                toast(friendlyError(reason), "error");
              } finally {
                setBusy(false);
              }
            }}
          />
        </div>
      )}
    </>
  );
}

const expiryOptions = [
  { id: "2592000", label: "30 days" },
  { id: "7776000", label: "90 days" },
  { id: "31536000", label: "1 year" },
  { id: "never", label: "Never" }
];

function CreateTokenModal({
  onClose,
  onCreated
}: {
  onClose: () => void;
  onCreated: (value: { token: Token; secret: string }) => void;
}) {
  const [name, setName] = useState("");
  const [repository, setRepository] = useState("");
  const [actions, setActions] = useState<string[]>(["pull"]);
  const [ttl, setTtl] = useState("7776000");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  function toggle(action: string) {
    if (action === "pull") return;
    setActions((current) => (current.includes(action) ? current.filter((item) => item !== action) : [...current, action]));
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    const expires_at = ttl === "never" ? undefined : Math.floor(Date.now() / 1000) + Number(ttl);
    try {
      const result = await api<Token & { secret: string }>("/api/v1/auth/tokens", {
        method: "POST",
        body: JSON.stringify({ name, scopes: [{ repository, actions }], expires_at })
      });
      onCreated({ token: result, secret: result.secret });
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  }
  const risky = actions.includes("delete") || actions.includes("admin");
  const expiresOn = ttl === "never" ? null : Math.floor(Date.now() / 1000) + Number(ttl);
  return (
    <Modal
      title="New access token"
      description="The secret is shown once, right after you create it."
      onClose={onClose}
      wide
      footer={
        <>
          <button className="btn ghost" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" form="create-token" disabled={busy || !name || !repository}>
            {busy ? "Creating…" : "Create token"}
          </button>
        </>
      }
    >
      <form id="create-token" className="form-grid" onSubmit={submit}>
        <label className="field">
          <span>Name</span>
          <input value={name} onChange={(event) => setName(event.target.value)} required autoFocus placeholder="e.g. github-actions-api" />
          <small>Something that tells you where it's used.</small>
        </label>
        <label className="field">
          <span>Repository</span>
          <input
            value={repository}
            onChange={(event) => setRepository(event.target.value.toLowerCase())}
            required
            placeholder="production/api"
            className="mono"
            spellCheck={false}
          />
          <small>One exact repository path. Wildcards are not accepted.</small>
        </label>
        <div className="field">
          <span>Permissions</span>
          <div className="choices">
            {["pull", "push", "delete", "admin"].map((action) => {
              const on = actions.includes(action);
              const danger = action === "delete" || action === "admin";
              return (
                <label key={action} className={`choice ${on ? "on" : ""} ${action === "pull" ? "locked" : ""}`}>
                  <input type="checkbox" checked={on} disabled={action === "pull"} onChange={() => toggle(action)} />
                  <div>
                    <strong>{action}</strong>
                    <small>{action === "pull" ? "Always included." : permissionHelp(action)}</small>
                  </div>
                  {danger && <AlertTriangle size={14} className="risk" style={{ color: "var(--warn)" }} aria-label="Sensitive" />}
                </label>
              );
            })}
          </div>
        </div>
        {risky && (
          <Callout tone="warn">
            <strong>Delete and admin can destroy images or change access.</strong> Grant them only to automation that needs them.
          </Callout>
        )}
        <div className="field">
          <span>Expiration</span>
          <div className="row">
            <Segmented label="Expiration" value={ttl} onChange={setTtl} options={expiryOptions} />
            <small className="muted">{expiresOn ? `Expires ${formatAbsolute(expiresOn).split(",").slice(0, 2).join(",")}` : "Never expires — revoke it manually."}</small>
          </div>
        </div>
        {error && <Callout tone="error">{error}</Callout>}
      </form>
    </Modal>
  );
}

function TokenSecretModal({
  host,
  username,
  secret,
  name,
  onClose
}: {
  host: string;
  username: string;
  secret: string;
  name: string;
  onClose: () => void;
}) {
  return (
    <Modal
      title={`“${name}” is ready`}
      description="Copy the secret now — you won't be able to see it again."
      onClose={onClose}
      footer={
        <button className="btn primary" onClick={onClose}>
          I've stored it safely
        </button>
      }
    >
      <div className="secret">
        <Lock size={14} className="muted" />
        <code>{secret}</code>
        <CopyButton className="btn sm" value={secret} label="Copy" />
      </div>
      <CommandBox label="Log in with it" command={`docker login ${host} -u ${username}`} />
      <p className="muted" style={{ fontSize: 12.5 }}>
        Paste the token when Docker prompts for a password. In CI, pipe it with <code>--password-stdin</code> instead of putting it on the command line.
      </p>
    </Modal>
  );
}

export function TokenDetailPage() {
  const { id } = useParams();
  const { host, user } = useSession();
  const { data, error, reload } = useResource<{ tokens: Token[] }>("/api/v1/auth/tokens");
  const navigate = useNavigate();
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton />;
  const token = data.tokens.find((item) => item.id === id);
  if (!token) {
    return (
      <EmptyState
        icon={<KeyRound size={18} />}
        title="Token not found"
        text="It may have been deleted, or it belongs to another account."
        action={
          <Link className="btn" to="/security/tokens">
            <ArrowLeft size={14} /> All tokens
          </Link>
        }
      />
    );
  }
  const state = tokenState(token);
  return (
    <>
      <PageHeader
        eyebrow={
          <Link className="inline-link" to="/security/tokens">
            Access tokens
          </Link>
        }
        title={token.name}
        badge={<StatusBadge dot state={state.tone} label={state.label} />}
        description={<span className="mono">{token.prefix}••••••••</span>}
        actions={!token.revoked_at && <RevokeButton large id={token.id} name={token.name} onDone={() => navigate("/security/tokens")} />}
      />
      <div className="grid">
        <Panel className="span-7" title="Repository access" description="What this token can do, as requested from the token service.">
          <div className="panel-body stack-lg">
            {token.namespace_pull && (
              <div>
                <Scopes token={{ ...token, scopes: [] }} />
                <div className="muted" style={{ fontSize: 12.5, marginTop: 6 }}>Pull from every repository in this namespace (Knotree Cloud grant).</div>
              </div>
            )}
            {token.scopes.map((scope) => (
              <div key={scope.repository} className="stack-sm">
                <div className="row" style={{ justifyContent: "space-between" }}>
                  <Link className="inline-link mono" to={`/repositories/${scope.repository}`}>
                    {scope.repository}
                  </Link>
                  <span className="row" style={{ gap: 4 }}>
                    {scope.actions.map((action) => (
                      <StatusBadge key={action} state={action === "delete" || action === "admin" ? "warn" : "neutral"} label={action} />
                    ))}
                  </span>
                </div>
                <code className="muted" style={{ fontSize: 12 }}>repository:{scope.repository}:{scope.actions.join(",")}</code>
              </div>
            ))}
          </div>
        </Panel>
        <Panel className="span-5" title="Details">
          <div className="panel-body">
            <dl className="kv compact">
              <dt>Prefix</dt>
              <dd className="mono">{token.prefix}</dd>
              <dt>Last used</dt>
              <dd>
                <RelativeTime value={token.last_used_at} />
              </dd>
              <dt>Expires</dt>
              <dd>{token.expires_at ? formatAbsolute(token.expires_at) : "Never"}</dd>
              {token.revoked_at && (
                <>
                  <dt>Revoked</dt>
                  <dd>{formatAbsolute(token.revoked_at)}</dd>
                </>
              )}
              <dt>ID</dt>
              <dd className="mono muted" style={{ fontSize: 12 }}>{token.id}</dd>
            </dl>
          </div>
        </Panel>
        {!token.revoked_at && (
          <Panel className="span-12" title="Use it" description="The secret is only shown at creation. If you lost it, revoke this token and create a new one.">
            <div className="panel-body">
              <CommandBox command={`echo "$KNOTREE_TOKEN" | docker login ${host} -u ${user.username} --password-stdin`} />
            </div>
          </Panel>
        )}
      </div>
    </>
  );
}
