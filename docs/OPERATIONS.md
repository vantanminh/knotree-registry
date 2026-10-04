# Operations runbook

## Production shape

The checked-in production compose profile runs PostgreSQL and a single `registry-server` container. The image builds and serves `web/dist` itself, so the Cloudflare Tunnel should target `http://127.0.0.1:8080`; no Vite development server belongs in the production path. Put TLS and request-size limits at the edge as well as in the application. Use `STORAGE_BACKEND=r2` with a private bucket when the host must be replaceable; local storage is supported only when the registry volume is backed up and the deployment stays single-host. The optional edge Worker uses a separate blob host and only receives short-lived digest-bound grants.

The service fails fast in `APP_ENV=production` unless the public URL is HTTPS, PostgreSQL is required, cookies are secure, memory storage is disabled, and `STATIC_ROOT/index.html` exists. It also requires Knotree Accounts sign-in (`SSO_ENABLED=true`); there is no local password sign-in or bootstrap administrator. Set `SSO_ADMIN_SUBJECTS` to the Knotree account ids that administer the instance.

## Backups and restore

PostgreSQL stores the runtime snapshot used to restore authorization, tags, local upload sessions, events, webhook registrations, pending webhook deliveries and garbage-collection reachability. R2 multipart upload internals are not resumed across a process restart; clients should retry an interrupted upload and the bucket should have a lifecycle rule for abandoned multipart uploads. Take a daily compressed dump and verify it in a disposable database:

```powershell
pg_dump --format=custom --file=registry-$(Get-Date -Format yyyyMMdd).dump $env:DATABASE_URL
pg_restore --clean --if-exists --dbname=$env:RESTORE_DATABASE_URL registry-20260922.dump
```

R2 contains immutable content-addressed bytes. Preserve the bucket and its lifecycle configuration; do not restore by making it public. Before deleting or recreating a bucket, run a metadata/object consistency check and keep the database dump and bucket snapshot together.

## Health and observability

- `/livez` only tests process liveness.
- `/readyz` checks storage and the configured database but only answers
  `{"status":"ok"}` (200) or `{"status":"not_ready"}` (503). Which component
  failed is logged as `registry not ready`.
- The cluster-only internal listener (`INTERNAL_BIND_ADDR`) serves operator
  detail: `/internal/v1/health` (storage, database, uptime, version) and
  `/metrics` (request and response-class counters, Prometheus text format).
  Neither is routed through the public ingress.

The public API and dashboard never describe the deployment: no storage
provider, bucket, blob-serving mode, environment, component health, uptime or
version. Storage and edge errors reach clients as generic messages and are
logged in full on the server.
- Every response includes `X-Request-Id`; pass a trusted incoming ID through the reverse proxy or let the service generate one.

Use structured tracing at `RUST_LOG=info`. Do not log `Authorization`, cookies, PATs, webhook secrets, passwords, or R2 credentials. Scrub reverse-proxy access logs for those headers too.

The webhook worker sends signed image and control-plane events from the
PostgreSQL-backed outbox. A 2xx response removes a delivery; transient
network/408/429/5xx failures retry up to six attempts, while other 4xx errors
are terminal. Monitor worker warnings and keep receivers idempotent by
`X-Knotree-Delivery`. The complete consumer contract and GitHub bridge are in
[`docs/DEPLOYMENT_NOTIFICATIONS.md`](DEPLOYMENT_NOTIFICATIONS.md).

## Maintenance

Abandoned in-memory upload sessions are marked aborted and their staging objects removed when a new upload begins. Database-backed deployments should also schedule cleanup for rows where `status = 'active'` and `expires_at < now()` and abort the corresponding multipart upload in R2.

Run administrator GC as a dry run first:

```powershell
Invoke-RestMethod -Method Post "$env:REGISTRY_URL/api/v1/admin/gc" -Headers @{ Cookie = $env:REGISTRY_SESSION } -ContentType 'application/json' -Body '{"dry_run":true,"grace_seconds":604800}'
```

Inspect failures before running the same request with `dry_run:false`. Never bypass repository authorization to make a digest visible; deduplicated bytes are still attached to repositories in metadata.

## Knotree accounts and the internal API

Web sign-in goes through Knotree Accounts (`SSO_ENABLED=true`). Registry
administrators are the Knotree account ids listed in `SSO_ADMIN_SUBJECTS`
(comma separated). The list is applied at every sign-in, so removing an id
removes the role the next time that person signs in. `docker login` keeps
using access tokens created in the web UI after signing in.

Knotree Cloud reads a user's images and gets image-pull credentials through a
cluster-only API, so users never connect the two services by hand:

- `INTERNAL_BIND_ADDR=0.0.0.0:8081` starts the listener. It is exposed only
  by the `registry-internal` Service (`deploy/k8s/internal.yaml`), never by
  the public ingress, and the `registry` NetworkPolicy admits port 8081 from
  the `knotree-cloud` namespace only.
- Cloud presents a projected ServiceAccount token with audience
  `knotree-registry-internal` (`INTERNAL_TOKEN_AUDIENCE`). Registry verifies it
  with the Kubernetes TokenReview API (the `registry` ServiceAccount is bound to
  `system:auth-delegator`). It accepts only the ServiceAccounts in
  `INTERNAL_ALLOWED_SERVICE_ACCOUNTS`, by default
  `system:serviceaccount:knotree-cloud:knotree-cloud-knotree-api`.
- Each request names the Knotree account (`X-Knotree-Issuer`,
  `X-Knotree-Subject`). Registry answers only for that account's own
  namespace, even when the account is an administrator.
- `POST /internal/v1/pull-credentials` issues a pull-only credential for one
  repository, valid for 90 days. Cloud rotates it, and Registry keeps the
  newest two live so pods that are still pulling are not cut off.

For local development without Kubernetes, set `INTERNAL_AUTH=dev-token` and an
`INTERNAL_DEV_TOKEN` of at least 32 characters. Production refuses this mode.

## Signing keys and token revocation

The active ES256 signing key and `kid` are included in the PostgreSQL runtime snapshot, so a normal restart does not invalidate browser sessions or short-lived registry tokens. The current snapshot is single-instance and last-write-wins; do not scale the registry horizontally. A future multi-instance rollout must replace the snapshot with transactional repositories and a key-ring backed by `registry_signing_keys` before adding replicas. Revoking a PAT immediately prevents new token minting; existing bearer tokens expire at the configured TTL.

## Incident response

1. Disable the affected webhook or revoke the affected PAT.
2. Rotate `EDGE_DOWNLOAD_SECRET`, R2 credentials, or webhook secrets through the secret manager; never commit replacements.
3. If JWT signing material is suspected, restart all registry instances together and invalidate any cached bearer tokens at the proxy.
4. Preserve request IDs, audit events, pending outbox entries, and the relevant PostgreSQL dump for investigation.
