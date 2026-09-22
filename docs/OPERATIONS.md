# Operations runbook

## Production shape

Run PostgreSQL, `registry-server`, a same-origin static host for `web/dist`, and a private Cloudflare R2 bucket. Put TLS and request-size limits at the reverse proxy as well as in the application. Keep `STORAGE_BACKEND=r2`, `R2_REGION=auto`, and never grant public bucket access. The optional edge Worker uses a separate `blobs.example.com` host and only receives short-lived digest-bound grants.

## Backups and restore

PostgreSQL metadata is required for authorization, tags, uploads, and garbage-collection reachability. Take a daily compressed dump and verify it in a disposable database:

```powershell
pg_dump --format=custom --file=registry-$(Get-Date -Format yyyyMMdd).dump $env:DATABASE_URL
pg_restore --clean --if-exists --dbname=$env:RESTORE_DATABASE_URL registry-20260922.dump
```

R2 contains immutable content-addressed bytes. Preserve the bucket and its lifecycle configuration; do not restore by making it public. Before deleting or recreating a bucket, run a metadata/object consistency check and keep the database dump and bucket snapshot together.

## Health and observability

- `/livez` only tests process liveness.
- `/readyz` checks storage and the configured database.
- `/metrics` exposes request and response-class counters in Prometheus text format.
- Every response includes `X-Request-Id`; pass a trusted incoming ID through the reverse proxy or let the service generate one.

Use structured tracing at `RUST_LOG=info`. Do not log `Authorization`, cookies, PATs, webhook secrets, passwords, or R2 credentials. Scrub reverse-proxy access logs for those headers too.

## Maintenance

Abandoned in-memory upload sessions are marked aborted and their staging objects removed when a new upload begins. Database-backed deployments should also schedule cleanup for rows where `status = 'active'` and `expires_at < now()` and abort the corresponding multipart upload in R2.

Run administrator GC as a dry run first:

```powershell
Invoke-RestMethod -Method Post "$env:REGISTRY_URL/api/v1/admin/gc" -Headers @{ Cookie = $env:REGISTRY_SESSION } -ContentType 'application/json' -Body '{"dry_run":true,"grace_seconds":604800}'
```

Inspect failures before running the same request with `dry_run:false`. Never bypass repository authorization to make a digest visible; deduplicated bytes are still attached to repositories in metadata.

## Signing keys and token revocation

Registry access JWT signing keys are process-local today and rotate when the service restarts; the short default TTL bounds already-issued tokens. For a multi-instance production rollout, persist active and retired keys in `registry_signing_keys`, publish a stable `kid`, overlap old and new verification keys for at least the token TTL, and retire the old key only after that overlap. Revoking a PAT immediately prevents new token minting; existing bearer tokens expire at the configured TTL.

## Incident response

1. Disable the affected webhook or revoke the affected PAT.
2. Rotate `EDGE_DOWNLOAD_SECRET`, R2 credentials, or webhook secrets through the secret manager; never commit replacements.
3. If JWT signing material is suspected, restart all registry instances together and invalidate any cached bearer tokens at the proxy.
4. Preserve request IDs, audit events, delivery history, and the relevant PostgreSQL dump for investigation.
