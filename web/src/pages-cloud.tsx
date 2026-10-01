import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowRight, CloudUpload, KeyRound, Rocket, ShieldCheck } from "lucide-react";
import { api, friendlyError } from "./api";
import { CommandBox, EmptyState, ErrorState, PageHeader, RelativeTime, Skeleton, StatusBadge } from "./ui";

type DeliveryRecord = {
  delivery_id: string;
  webhook_id: string;
  event: string;
  repository?: string | null;
  tag?: string | null;
  digest?: string | null;
  attempt: number;
  status?: number | null;
  outcome: "delivered" | "retrying" | "failed";
  error?: string | null;
  at: number;
};

type CloudIntegration = {
  configured: boolean;
  webhook?: { id: string; url: string; events: string[]; enabled: boolean; created_at: number } | null;
  pending: number;
  deliveries: DeliveryRecord[];
  public_url: string;
};

const outcomeBadge: Record<DeliveryRecord["outcome"], { state: "ok" | "warn" | "fail"; label: string }> = {
  delivered: { state: "ok", label: "Delivered" },
  retrying: { state: "warn", label: "Retrying" },
  failed: { state: "fail", label: "Failed" }
};

function connectionState(data: CloudIntegration): { state: "ok" | "warn" | "fail" | "neutral"; label: string; text: string } {
  if (!data.configured) {
    return {
      state: "neutral",
      label: "Not configured",
      text: "Registry is not sending push events to Knotree Cloud. An operator sets CLOUD_WEBHOOK_URL and KNOTREE_REGISTRY_WEBHOOK_SECRET on this deployment."
    };
  }
  const last = data.deliveries[0];
  if (!last) return { state: "ok", label: "Connected", text: "Waiting for the first image push. Every tag update is sent to Cloud, signed and retried until it is accepted." };
  if (last.outcome === "delivered") return { state: "ok", label: "Connected", text: "Cloud accepted the latest push. Services that follow the repository deploy its digest automatically." };
  if (last.outcome === "retrying") return { state: "warn", label: "Retrying", text: "Cloud did not accept the latest delivery yet. Registry retries with backoff for about an hour." };
  return { state: "fail", label: "Delivery failing", text: "Cloud rejected the latest delivery. Check that both sides share the same signing secret." };
}

export function CloudConnectionPage({ admin, host }: { admin: boolean; host: string }) {
  const [data, setData] = useState<CloudIntegration | null>(null);
  const [error, setError] = useState("");
  function load() {
    setError("");
    api<CloudIntegration>("/api/v1/integrations/cloud")
      .then(setData)
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(() => {
    if (admin) load();
  }, [admin]);

  return (
    <>
      <PageHeader
        title="Knotree Cloud"
        description="Push an image here and Knotree Cloud pulls it and deploys the exact digest to every service that follows the repository."
        actions={admin ? <button className="btn" onClick={load}>Refresh</button> : undefined}
      />
      <section className="panel panel-pad cloud-flow" aria-label="How auto deploy works">
        <FlowStep icon={<CloudUpload size={18} />} title="Push" text={`docker push ${host}/team/app:latest`} />
        <ArrowRight className="cloud-flow-arrow" size={18} aria-hidden="true" />
        <FlowStep icon={<ShieldCheck size={18} />} title="Signed event" text="HMAC-signed tag_updated, retried until accepted" />
        <ArrowRight className="cloud-flow-arrow" size={18} aria-hidden="true" />
        <FlowStep icon={<Rocket size={18} />} title="Deploy" text="Cloud deploys image@sha256 with its pull token" />
      </section>

      {admin && error && <ErrorState message={error} onRetry={load} />}
      {admin && !error && !data && <Skeleton rows={4} />}
      {admin && data && <ConnectionStatus data={data} />}

      <section className="panel" style={{ marginBottom: 16 }}>
        <div className="panel-head">
          <div>
            <h2>Connect a service</h2>
            <p>Each Cloud service follows one repository with its own pull-only token.</p>
          </div>
        </div>
        <ol className="cloud-steps">
          <li>
            <span className="cloud-step-num">1</span>
            <div>
              <strong>Create a pull token</strong>
              <p>
                In <Link to="/security/tokens">Access Tokens</Link>, create a token scoped to the repository with only <code>pull</code>. Copy the secret.
              </p>
            </div>
          </li>
          <li>
            <span className="cloud-step-num">2</span>
            <div>
              <strong>Connect it in Cloud</strong>
              <p>
                Open the service in Knotree Cloud, choose <em>Connect repository</em>, enter <code>team/app</code>, the tag to follow and the token.
              </p>
            </div>
          </li>
          <li>
            <span className="cloud-step-num">3</span>
            <div>
              <strong>Push</strong>
              <p>The next push of that tag deploys automatically. Pause or disconnect it from the same panel.</p>
              <CommandBox command={`docker push ${host}/team/app:latest`} />
            </div>
          </li>
        </ol>
        <div className="panel-pad" style={{ paddingTop: 0 }}>
          <a className="btn primary" href="https://cloud.knotree.com/integrations" target="_blank" rel="noreferrer">
            <KeyRound size={14} /> Open Cloud integrations
          </a>
        </div>
      </section>
    </>
  );
}

function FlowStep({ icon, title, text }: { icon: React.ReactNode; title: string; text: string }) {
  return (
    <div className="cloud-flow-step">
      <span className="cloud-flow-icon">{icon}</span>
      <div>
        <strong>{title}</strong>
        <span>{text}</span>
      </div>
    </div>
  );
}

function ConnectionStatus({ data }: { data: CloudIntegration }) {
  const status = connectionState(data);
  const delivered = data.deliveries.filter((d) => d.outcome === "delivered").length;
  const failed = data.deliveries.filter((d) => d.outcome === "failed").length;
  return (
    <>
      <section className="panel panel-pad" style={{ marginBottom: 16 }}>
        <div className="cloud-status">
          <div>
            <StatusBadge state={status.state} label={status.label} />
            <p>{status.text}</p>
          </div>
          {data.webhook && <code className="cloud-endpoint" title={data.webhook.url}>{data.webhook.url}</code>}
        </div>
        <div className="meta-row" style={{ marginTop: 16, marginBottom: 0 }}>
          <div><span>Events</span><strong>{data.webhook?.events.join(", ") || "—"}</strong></div>
          <div><span>Queued</span><strong>{data.pending}</strong></div>
          <div><span>Delivered (recent)</span><strong>{delivered}</strong></div>
          <div><span>Failed (recent)</span><strong>{failed}</strong></div>
          <div><span>Last attempt</span><strong><RelativeTime value={data.deliveries[0]?.at} /></strong></div>
        </div>
      </section>
      <section className="panel" style={{ marginBottom: 16 }}>
        <div className="panel-head">
          <div>
            <h2>Recent deliveries</h2>
            <p>Every attempt to notify Knotree Cloud, newest first.</p>
          </div>
        </div>
        {data.deliveries.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>When</th>
                  <th>Image</th>
                  <th>Result</th>
                  <th className="num">HTTP</th>
                  <th className="num">Attempt</th>
                </tr>
              </thead>
              <tbody>
                {data.deliveries.map((d) => (
                  <tr key={`${d.delivery_id}-${d.attempt}`}>
                    <td><RelativeTime value={d.at} /></td>
                    <td className="mono">{d.repository ?? "—"}{d.tag ? `:${d.tag}` : ""}</td>
                    <td>
                      <StatusBadge state={outcomeBadge[d.outcome].state} label={outcomeBadge[d.outcome].label} />
                      {d.error && <span className="muted" style={{ marginLeft: 8 }} title={d.error}>network error</span>}
                    </td>
                    <td className="num mono">{d.status ?? "—"}</td>
                    <td className="num">{d.attempt}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState title="No deliveries yet" text="Push a tag to see Registry notify Knotree Cloud here." />
        )}
      </section>
    </>
  );
}
