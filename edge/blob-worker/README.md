# Private blob edge

This Worker is an optional high-throughput pull path. The Rust registry still authorizes the repository and digest first; with `PULL_MODE=edge`, it returns a short-lived `307` grant for the immutable blob key. The Worker validates the HMAC grant, reads the private R2 binding, and serves `GET`, `HEAD`, and single-range `GET` responses with `Docker-Content-Digest`, `ETag`, `Content-Length`, `Content-Range`, and `Accept-Ranges`.

The bucket is never listed or made public. Set the grant secret out of band and use the same value for the registry’s `EDGE_DOWNLOAD_SECRET`:

```powershell
npx wrangler secret put DOWNLOAD_GRANT_SECRET
npx wrangler deploy
```

The generated `worker-configuration.d.ts` is produced by `npm run generate-types` and should be regenerated after binding changes. Validate locally with:

```powershell
npm install
npm run generate-types
npm run typecheck
npm test
npx wrangler deploy --dry-run
```

Use an edge URL such as `https://blobs.example.com/v1/blob` for `EDGE_DOWNLOAD_URL`. Keep the default `PULL_MODE=proxy` until the Worker is deployed and an authenticated Docker/containerd compatibility check has passed.
