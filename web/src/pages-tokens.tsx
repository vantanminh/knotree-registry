import { FormEvent, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { Token, api, friendlyError } from "./api";
import {
  CommandBox,
  ConfirmDialog,
  CopyButton,
  EmptyState,
  ErrorState,
  Modal,
  PageHeader,
  RelativeTime,
  SearchInput,
  Skeleton,
  StatusBadge,
  permissionHelp,
  useToast
} from "./ui";

export function TokensPage({ host, username }: { host: string; username: string }) {
  const [tokens, setTokens] = useState<Token[] | null>(null);
  const [error, setError] = useState("");
  const [secret, setSecret] = useState<{ token: Token; secret: string } | null>(null);
  const [params, setParams] = useSearchParams();
  const create = params.get("create") === "1";
  const navigate = useNavigate();
  const query = params.get("q") ?? "";
  function load() {
    api<{ tokens: Token[] }>("/api/v1/auth/tokens")
      .then((result) => setTokens(result.tokens))
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  const filtered = useMemo(
    () =>
      (tokens ?? []).filter(
        (token) =>
          !query ||
          token.name.toLowerCase().includes(query.toLowerCase()) ||
          token.prefix.includes(query) ||
          token.scopes.some((scope) => scope.repository.includes(query))
      ),
    [tokens, query]
  );
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!tokens) return <Skeleton />;
  return (
    <>
      <PageHeader
        title="Access Tokens"
        description="Create credentials for Docker CLI, CI systems and servers."
        actions={
          <button className="btn primary" onClick={() => setParams({ create: "1" })}>
            Create Token
          </button>
        }
      />
      <div className="filters">
        <SearchInput
          value={query}
          onChange={(value) => {
            const next = new URLSearchParams(params);
            if (value) next.set("q", value);
            else next.delete("q");
            setParams(next, { replace: true });
          }}
          placeholder="Search tokens"
        />
      </div>
      <section className="panel">
        {filtered.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Type</th>
                  <th>Scopes</th>
                  <th>Expires</th>
                  <th>Last used</th>
                  <th>Status</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((token) => (
                  <tr key={token.id} onClick={() => navigate(`/security/tokens/${token.id}`)}>
                    <td>
                      <strong>{token.name}</strong>
                      <div className="mono" style={{ color: "var(--muted)" }}>
                        {token.prefix}••••••••
                      </div>
                    </td>
                    <td>Personal</td>
                    <td>
                      {token.scopes.map((scope) => (
                        <span className="chip" key={scope.repository}>
                          {scope.repository} · {scope.actions.join(" ")}
                        </span>
                      ))}
                    </td>
                    <td>
                      <RelativeTime value={token.expires_at} />
                    </td>
                    <td>
                      <RelativeTime value={token.last_used_at} />
                    </td>
                    <td>
                      <StatusBadge state={token.revoked_at ? "fail" : "ok"} label={token.revoked_at ? "Revoked" : "Active"} />
                    </td>
                    <td className="row-actions">{!token.revoked_at && <RevokeButton id={token.id} onDone={load} />}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState
            title="No access tokens"
            text="Create a token to authenticate Docker, CI pipelines or deployment servers."
            action={
              <button className="btn primary" onClick={() => setParams({ create: "1" })}>
                Create token
              </button>
            }
          />
        )}
      </section>
      {create && (
        <CreateTokenModal
          onClose={() => {
            params.delete("create");
            setParams(params, { replace: true });
          }}
          onCreated={(created) => {
            params.delete("create");
            setParams(params, { replace: true });
            setSecret(created);
            load();
          }}
        />
      )}
      {secret && <TokenSecretModal host={host} username={username} secret={secret.secret} onClose={() => setSecret(null)} />}
    </>
  );
}

function RevokeButton({ id, onDone }: { id: string; onDone: () => void }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  return (
    <>
      <button
        className="btn sm danger"
        onClick={(event) => {
          event.stopPropagation();
          setOpen(true);
        }}
      >
        Revoke
      </button>
      {open && (
        <ConfirmDialog
          title="Revoke access token?"
          text="Applications using this token will no longer be able to request new registry credentials."
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
      )}
    </>
  );
}

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
  const [ttl, setTtl] = useState("never");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  function toggle(action: string, enabled: boolean) {
    setActions((current) => {
      const next = enabled ? [...new Set([...current, action])] : current.filter((item) => item !== action);
      return next.includes("pull") ? next : ["pull", ...next];
    });
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    const expires_at = ttl === "never" ? undefined : Math.floor(Date.now() / 1000) + Number(ttl);
    try {
      const result = await api<Token & { secret: string }>("/api/v1/auth/tokens", {
        method: "POST",
        body: JSON.stringify({
          name,
          scopes: [{ repository, actions }],
          expires_at
        })
      });
      onCreated({ token: result, secret: result.secret });
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title="Create access token"
      onClose={onClose}
      wide
      footer={
        <>
          <button className="btn ghost" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" form="create-token" disabled={busy}>
            {busy ? "Creating…" : "Create token"}
          </button>
        </>
      }
    >
      <form id="create-token" className="stack" onSubmit={submit}>
        <label className="field">
          <span>Token name</span>
          <input value={name} onChange={(event) => setName(event.target.value)} required />
        </label>
        <label className="field">
          <span>Token type</span>
          <select defaultValue="personal">
            <option value="personal">Personal</option>
            <option value="robot" disabled>
              Robot (not available)
            </option>
          </select>
        </label>
        <label className="field">
          <span>Repository</span>
          <small>Use a repository name such as production/api. Wildcards are not accepted by the control plane.</small>
          <input value={repository} onChange={(event) => setRepository(event.target.value)} required placeholder="lowercase/name" />
        </label>
        <div className="perm">
          {["pull", "push", "delete", "admin"].map((action) => (
            <label key={action} className={`perm-item ${action === "delete" || action === "admin" ? "warn" : ""}`}>
              <input
                type="checkbox"
                checked={actions.includes(action)}
                disabled={action === "pull"}
                onChange={(event) => toggle(action, event.target.checked)}
              />
              <div>
                <strong style={{ textTransform: "capitalize" }}>{action}</strong>
                <div style={{ color: "var(--muted)", fontSize: 13 }}>{permissionHelp(action)}</div>
              </div>
            </label>
          ))}
        </div>
        {(actions.includes("delete") || actions.includes("admin")) && (
          <div className="alert warn">Delete and admin permissions can destroy images or change access. Grant them only when required.</div>
        )}
        <label className="field">
          <span>Expiration</span>
          <select value={ttl} onChange={(event) => setTtl(event.target.value)}>
            <option value="never">No expiration</option>
            <option value="86400">1 day</option>
            <option value="604800">7 days</option>
            <option value="2592000">30 days</option>
            <option value="31536000">1 year</option>
          </select>
        </label>
        {error && <div className="alert error">{error}</div>}
      </form>
    </Modal>
  );
}

function TokenSecretModal({
  host,
  username,
  secret,
  onClose
}: {
  host: string;
  username: string;
  secret: string;
  onClose: () => void;
}) {
  return (
    <Modal
      title="Token created"
      onClose={onClose}
      footer={
        <button className="btn primary" onClick={onClose}>
          I've saved my token
        </button>
      }
    >
      <div className="alert warn">This token will only be shown once. Copy it now and store it somewhere safe.</div>
      <div className="secret-box">{secret}</div>
      <CopyButton value={secret} label="Copy token" />
      <h3>Docker login</h3>
      <CommandBox command={`docker login ${host} -u ${username}`} />
      <p style={{ color: "var(--muted)", fontSize: 13 }}>Paste the token when Docker asks for your password. Do not put the secret on the command line.</p>
    </Modal>
  );
}

export function TokenDetailPage() {
  const { id } = useParams();
  const [token, setToken] = useState<Token | null>(null);
  const [error, setError] = useState("");
  const navigate = useNavigate();
  function load() {
    api<{ tokens: Token[] }>("/api/v1/auth/tokens")
      .then((result) => setToken(result.tokens.find((item) => item.id === id) ?? null))
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, [id]);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!token) return <Skeleton />;
  return (
    <>
      <PageHeader
        title={token.name}
        description={`${token.prefix}••••••••`}
        actions={!token.revoked_at && <RevokeButton id={token.id} onDone={() => navigate("/security/tokens")} />}
      />
      <section className="panel panel-pad grid-gap">
        <div className="meta-row">
          <div>
            <span>Status</span>
            <StatusBadge state={token.revoked_at ? "fail" : "ok"} label={token.revoked_at ? "Revoked" : "Active"} />
          </div>
          <div>
            <span>Expires</span>
            <strong>
              <RelativeTime value={token.expires_at} />
            </strong>
          </div>
          <div>
            <span>Last used</span>
            <strong>
              <RelativeTime value={token.last_used_at} />
            </strong>
          </div>
        </div>
        <div>
          <h3>Repository access</h3>
          {token.scopes.map((scope) => (
            <div key={scope.repository} style={{ marginTop: 12 }}>
              <span className="chip">{scope.repository}</span>
              {scope.actions.map((action) => (
                <span className="chip" key={action}>
                  {action}
                </span>
              ))}
              <details className="details" style={{ marginTop: 8 }}>
                <summary>Advanced</summary>
                <code>repository:{scope.repository}:{scope.actions.join(",")}</code>
              </details>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
