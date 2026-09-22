# Control plane

The registry keeps browser management under `/api/v1` and leaves the Docker/OCI protocol under `/v2`. Browser state is carried by an `HttpOnly`, `SameSite=Strict` `kntr_session` cookie; production deployments should enable `COOKIE_SECURE=true` and serve the dashboard over TLS.

## Development UI

```powershell
cd web
npm install
npm run dev
```

The Vite development server proxies `/api`, `/auth`, and `/v2` to `http://127.0.0.1:8080`. The production bundle is generated with `npm run build`; serve `web/dist` from the same origin as the registry API or configure an equivalent reverse-proxy path.

## Endpoints

| Endpoint | Purpose |
| --- | --- |
| `POST /api/v1/auth/login` | Create a browser session from the bootstrap username/password. |
| `POST /api/v1/auth/logout` | Revoke the current browser session. |
| `GET /api/v1/auth/me` | Return the current user summary. |
| `GET /api/v1/overview` | Return the current user, repository count, repository names, and active credential count. |
| `GET /api/v1/repositories` | List private repositories visible to the current session. |
| `GET /api/v1/repositories/{name}` | Return tag, digest, media type, size, and creation metadata. Nested names retain their `/` separator. |
| `GET /api/v1/auth/tokens` | List credential metadata without secrets. |
| `POST /api/v1/auth/tokens` | Create a scoped credential; the generated secret is returned once. |
| `POST /api/v1/auth/tokens/{id}/revoke` | Revoke a credential owned by the current user or by an administrator. |
| `POST /api/v1/admin/gc` | Run or dry-run mark-and-sweep garbage collection as an administrator. |

Credential secrets are never returned by list endpoints. Action scopes are `pull`, `push`, `delete`, and `admin`; requested scopes are checked against the logged-in user’s credential before they reach the Docker Bearer token service.

The current dashboard covers sign-in, overview, repository browsing, one-time token reveal/revoke, and security guidance. Namespace membership, audit history, webhooks, and agent settings remain separate service capabilities so they can be added without mixing browser cookies into the `/v2` protocol.
