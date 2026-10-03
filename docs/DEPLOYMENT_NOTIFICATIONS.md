# Image-publish deployment notifications

Knotree Registry can notify an external deployment service after an OCI
manifest is committed. The registry does not execute commands on the target
service; the receiver decides whether and how to deploy. A receiver should
pull the immutable digest in the event instead of resolving a mutable tag
again.

## Create a deployment webhook

Create the endpoint as an administrator. The secret is returned only once:

```bash
curl -sS -X POST "$REGISTRY_URL/api/v1/webhooks" \
  -H "Cookie: $REGISTRY_SESSION" \
  -H 'Content-Type: application/json' \
  -d '{"url":"https://deploy.example.com/hooks/knotree","events":["tag_updated"]}'
```

Use `tag_updated` as the deployment trigger. `manifest_pushed` is emitted for
both digest and tag publication and is useful for artifact indexing; a tagged
push emits both events. An empty `events` array subscribes to every event.
Production endpoints should be HTTPS. Disable and recreate a webhook to rotate
its secret.

## Delivery contract

Every request is a JSON `RegistryEvent` with `schema_version: 1`:

```json
{
  "schema_version": 1,
  "id": "event-uuid",
  "kind": "tag_updated",
  "occurred_at": 1760000000,
  "actor": null,
  "repository": "team/app",
  "tag": "stable",
  "digest": "sha256:…",
  "metadata": {
    "registry": "registry.example.com",
    "immutable_image": "registry.example.com/team/app@sha256:…",
    "tagged_image": "registry.example.com/team/app:stable",
    "manifest_url": "https://registry.example.com/v2/team/app/manifests/sha256:…",
    "media_type": "application/vnd.oci.image.manifest.v1+json",
    "size": 421,
    "published_reference": "stable",
    "is_tag": true,
    "namespace": "team",
    "owner_issuer": "https://accounts.knotree.com",
    "owner_subject": "accounts-user-uuid"
  }
}
```

`tag_updated` events carry the repository `namespace`. When the namespace
owner signed in through Knotree accounts SSO, they also carry `owner_issuer`
and `owner_subject` (the accounts `sub`). Knotree Cloud routes auto-deploys
for account connections only when these match the connected Cloud user, so a
repository name alone is never trusted as proof of ownership.

The operator-managed Cloud webhook also receives `grant_revoked` when a user
revokes a credential that was issued to Knotree Cloud through the retired
consent grant (a namespace connection or a repository authorization). New
Cloud credentials come from the internal API and are renewed by Cloud. Its metadata holds
`credential_id`, `namespace` and, for SSO users, `owner_issuer` and
`owner_subject`. Cloud marks the matching connection revoked and turns off
auto-deploy for the apps that used it. Revoking an ordinary access token does
not emit this event.

The following headers are sent with the exact raw body used for signing:

| Header | Meaning |
| --- | --- |
| `X-Knotree-Event` | Event kind, for example `tag_updated` |
| `X-Knotree-Delivery` | Unique delivery UUID; use it as the idempotency key |
| `X-Knotree-Timestamp` | Unix seconds used in the signature |
| `X-Knotree-Signature` | `sha256=` HMAC-SHA256 signature |
| `X-Knotree-Attempt` | 1-based attempt number |

The signature is calculated as:

```text
HMAC-SHA256(webhook_secret, `${timestamp}.${delivery_id}.${raw_body}`)
```

Accept a delivery only when the timestamp is within five minutes, the
signature is constant-time valid, and the delivery UUID has not already been
processed. Acknowledge with any 2xx response after recording the deployment
job. Pull the image using `metadata.immutable_image` (or
`repository@digest`) so a later tag mutation cannot change the rollout.

The registry keeps an outbox in the PostgreSQL runtime snapshot and sends
deliveries from one worker per registry instance. It retries network errors,
timeouts, 408, 429 and 5xx responses up to six attempts with exponential
backoff (2s, 4s, 8s, 16s and 32s). Other 4xx responses are permanent
failures, as are redirects. The contract is at-least-once, so receivers must
be idempotent.

## GitHub Actions bridge

If a platform only deploys from GitHub (the common Railway-style workflow),
place a small receiver in front of GitHub. After verifying the Knotree
signature, it can call GitHub's `repository_dispatch` API with the digest as
data. The deployment repository can then use a workflow like this:

```yaml
name: deploy-knotree-image

on:
  repository_dispatch:
    types: [knotree-image-published]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - name: Log in to the registry
        uses: docker/login-action@v3
        with:
          registry: registry.example.com
          username: ${{ secrets.KNOTREE_PULL_USERNAME }}
          password: ${{ secrets.KNOTREE_PULL_SECRET }}
      - name: Pull the immutable image
        env:
          IMAGE: ${{ github.event.client_payload.image }}
        run: docker pull "$IMAGE"
      - name: Deploy
        env:
          IMAGE: ${{ github.event.client_payload.image }}
        run: ./deploy.sh "$IMAGE"
```

The bridge should pass `metadata.immutable_image`, `repository`, `tag`, and
`digest` as `client_payload`; it should never pass the webhook secret or a
mutable tag as the deployment selector. This keeps GitHub/Railway-style source
deploys separate from registry authentication and makes a failed deploy
retryable without republishing the image.
