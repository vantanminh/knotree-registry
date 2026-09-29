import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { api, friendlyError } from "./api";

type Consent = {
  client: string;
  repository: string;
  username: string;
  credential_lifetime_days: number;
};

export function CloudAuthorizationPage() {
  const { requestId } = useParams();
  const [consent, setConsent] = useState<Consent | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    setConsent(null);
    setError("");
    api<Consent>(`/api/v1/cloud-grants/requests/${encodeURIComponent(requestId ?? "")}`, { signal: controller.signal })
      .then((value) => { if (!controller.signal.aborted) setConsent(value); })
      .catch((reason) => { if (!controller.signal.aborted) setError(friendlyError(reason)); });
    return () => controller.abort();
  }, [requestId]);

  async function decide(allow: boolean) {
    if (!consent || busy) return;
    setBusy(true);
    setError("");
    try {
      const result = await api<{ redirect_url: string }>(`/api/v1/cloud-grants/requests/${encodeURIComponent(requestId ?? "")}/decision`, {
        method: "POST",
        body: JSON.stringify({ allow })
      });
      const target = new URL(result.redirect_url);
      if (target.origin !== "https://cloud.knotree.com" || target.pathname !== "/api/v1/auth/knotree-registry/callback" || target.username || target.password || target.hash) {
        throw new Error("The authorization callback is invalid.");
      }
      window.location.assign(target.href);
    } catch (reason) {
      setError(friendlyError(reason));
      setBusy(false);
    }
  }

  return (
    <main className="auth-shell">
      <section className="auth-card stack" aria-busy={busy}>
        <div className="brand-mark">K</div>
        <h1>Connect to Cloud</h1>
        {!consent && !error && <p role="status">Loading authorization request…</p>}
        {consent && <>
          <p>{consent.client} requests permission to pull images from:</p>
          <strong style={{ overflowWrap: "anywhere" }}>{consent.repository}</strong>
          <p>Signed in as {consent.username}. Access lasts {consent.credential_lifetime_days} days. You can revoke it in Registry Access Tokens.</p>
          <p>Cloud can download images from this repository. This permission does not allow pushing or deleting images.</p>
          <button className="btn primary full" disabled={busy} onClick={() => decide(true)}>{busy ? "Processing…" : "Allow pull access"}</button>
          <button className="btn full" disabled={busy} onClick={() => decide(false)}>Deny</button>
        </>}
        {error && <div className="alert error" role="alert">{error}</div>}
      </section>
    </main>
  );
}
