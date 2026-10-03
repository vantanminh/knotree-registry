import { Link } from "react-router-dom";
import { ArrowRight, CloudUpload, ExternalLink, Rocket, Send, ShieldCheck } from "lucide-react";
import { useResource } from "./api";
import { useSession } from "./session";
import {
  CommandBox,
  EmptyState,
  ErrorState,
  PageHeader,
  Panel,
  RelativeTime,
  Skeleton,
  Stats,
  StatusBadge,
  Tone
} from "./ui";

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

const outcomeBadge: Record<DeliveryRecord["outcome"], { state: Tone; label: string }> = {
  delivered: { state: "ok", label: "Delivered" },
  retrying: { state: "warn", label: "Retrying" },
  failed: { state: "fail", label: "Failed" }
};

function connectionState(data: CloudIntegration): { state: Tone; label: string; text: string } {
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

export function CloudConnectionPage() {
  const { user, host } = useSession();
  const admin = user.is_admin;
  const { data, error, reload } = useResource<CloudIntegration>(admin ? "/api/v1/integrations/cloud" : null);

  return (
    <>
      <PageHeader
        title="Knotree Cloud"
        description="Push here and Knotree Cloud deploys the exact digest to every service that follows the repository."
        actions={
          <>
            {admin && (
              <button className="btn" onClick={reload}>
                Refresh
              </button>
            )}
            <a className="btn primary" href="https://cloud.knotree.com/integrations" target="_blank" rel="noreferrer">
              Open Cloud <ExternalLink size={13} />
            </a>
          </>
        }
      />

      <section className="panel flow" aria-label="How auto deploy works" style={{ marginBottom: 16 }}>
        <FlowStep icon={<CloudUpload size={16} />} title="You push" text={`docker push ${host}/team/app:latest`} mono />
        <div className="flow-arrow"><ArrowRight size={16} /></div>
        <FlowStep icon={<ShieldCheck size={16} />} title="Registry signs an event" text="HMAC-signed tag_updated, retried until Cloud accepts it." />
        <div className="flow-arrow"><ArrowRight size={16} /></div>
        <FlowStep icon={<Rocket size={16} />} title="Cloud deploys the digest" text="image@sha256:… pulled with the service's own read-only token." />
      </section>

      {admin && error && (
        <section className="panel" style={{ marginBottom: 16 }}>
          <ErrorState message={error} onRetry={reload} />
        </section>
      )}
      {admin && !error && !data && <Skeleton rows={3} stats />}
      {admin && data && <ConnectionStatus data={data} />}

      <Panel title="Connect a service" description="Each Cloud service follows one repository with its own pull-only token.">
        <div className="panel-body">
          <ol className="steps">
            <li>
              <div>
                <strong>Create a pull-only token</strong>
                <p>
                  In <Link className="inline-link" to="/security/tokens?create=1">Access tokens</Link>, scope a token to the repository with just <code>pull</code>.
                </p>
              </div>
            </li>
            <li>
              <div>
                <strong>Connect the repository in Cloud</strong>
                <p>
                  Open the service, choose <em>Connect repository</em>, then enter <code>team/app</code>, the tag to follow and the token.
                </p>
              </div>
            </li>
            <li>
              <div>
                <strong>Push</strong>
                <p>The next push of that tag deploys automatically. Pause or disconnect from the same panel in Cloud.</p>
                <CommandBox command={`docker push ${host}/team/app:latest`} />
              </div>
            </li>
          </ol>
        </div>
      </Panel>
    </>
  );
}

function FlowStep({ icon, title, text, mono }: { icon: React.ReactNode; title: string; text: string; mono?: boolean }) {
  return (
    <div className="flow-step">
      <span className="icon">{icon}</span>
      <strong>{title}</strong>
      <p className={mono ? "mono" : undefined} style={mono ? { overflowWrap: "anywhere" } : undefined}>{text}</p>
    </div>
  );
}

function ConnectionStatus({ data }: { data: CloudIntegration }) {
  const status = connectionState(data);
  const delivered = data.deliveries.filter((d) => d.outcome === "delivered").length;
  const failed = data.deliveries.filter((d) => d.outcome === "failed").length;
  return (
    <>
      <section className="panel" style={{ marginBottom: 16 }}>
        <div className="panel-head bordered">
          <div style={{ minWidth: 0 }}>
            <h2 className="row" style={{ gap: 10 }}>
              Connection <StatusBadge dot state={status.state} label={status.label} />
            </h2>
            <p style={{ maxWidth: 640 }}>{status.text}</p>
          </div>
        </div>
        {data.webhook && (
          <div className="panel-body">
            <dl className="kv compact">
              <dt>Endpoint</dt>
              <dd className="mono">{data.webhook.url}</dd>
              <dt>Events</dt>
              <dd>{data.webhook.events.map((event) => <span key={event} className="tag-pill" style={{ marginRight: 4 }}>{event}</span>)}</dd>
            </dl>
          </div>
        )}
      </section>
      {data.configured && (
        <Stats
          items={[
            { label: "Queued", value: data.pending, detail: "Waiting to send" },
            { label: "Delivered", value: delivered, detail: "Recent attempts" },
            { label: "Failed", value: failed, detail: "Recent attempts" },
            { label: "Last attempt", value: <RelativeTime value={data.deliveries[0]?.at} /> }
          ]}
        />
      )}
      <Panel title="Recent deliveries" description="Every attempt to notify Knotree Cloud, newest first." className="" >
        {data.deliveries.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Image</th>
                  <th>Result</th>
                  <th className="num">HTTP</th>
                  <th className="num hide-sm">Attempt</th>
                  <th className="num">When</th>
                </tr>
              </thead>
              <tbody>
                {data.deliveries.map((d) => (
                  <tr key={`${d.delivery_id}-${d.attempt}`}>
                    <td className="mono">
                      {d.repository ?? "—"}
                      {d.tag ? <span className="muted">:{d.tag}</span> : ""}
                    </td>
                    <td>
                      <span className="row" style={{ gap: 8, flexWrap: "nowrap" }}>
                        <StatusBadge dot state={outcomeBadge[d.outcome].state} label={outcomeBadge[d.outcome].label} />
                        {d.error && <span className="muted" style={{ fontSize: 12 }} title={d.error}>network error</span>}
                      </span>
                    </td>
                    <td className="num mono">{d.status ?? "—"}</td>
                    <td className="num hide-sm">#{d.attempt}</td>
                    <td className="num muted">
                      <RelativeTime value={d.at} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState icon={<Send size={18} />} title="No deliveries yet" text="Push a tag to see Registry notify Knotree Cloud here." />
        )}
      </Panel>
      <div style={{ height: 16 }} />
    </>
  );
}
