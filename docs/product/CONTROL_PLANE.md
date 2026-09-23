# Control plane

The registry keeps browser management under `/api/v1` and leaves the Docker/OCI protocol under `/v2`. Browser state is carried by an `HttpOnly`, `SameSite=Strict` `kntr_session` cookie; production deployments should enable `COOKIE_SECURE=true` and serve the dashboard over TLS.

## Development UI

```powershell
cd web
npm install
npm run dev
```

The Vite development server proxies `/api`, `/auth`, and `/v2` to `http://127.0.0.1:8080`. The production image runs `npm run build` during the container build and the Rust service serves `STATIC_ROOT` from the same origin as the registry API. Keep Vite only in the local development workflow.

## Endpoints

| Endpoint | Purpose |
| --- | --- |
| `POST /api/v1/auth/login` | Create a browser session from the bootstrap username/password; include the six-digit `otp` value when TOTP is enabled. |
| `POST /api/v1/auth/logout` | Revoke the current browser session. |
| `GET /api/v1/auth/me` | Return the current user summary. |
| `POST /api/v1/auth/password` | Change the current user password; requires the current password and confirmation, then rotates the browser session. |
| `GET /api/v1/auth/2fa` | Return whether authenticator-app TOTP is enabled. |
| `POST /api/v1/auth/2fa/setup` | Verify the current password and start TOTP setup; returns the one-time setup secret and `otpauth://` URI. |
| `POST /api/v1/auth/2fa/confirm` | Confirm a six-digit authenticator-app code and enable TOTP. |
| `POST /api/v1/auth/2fa/disable` | Disable TOTP after verifying the current password and authenticator-app code. |
| `GET /api/v1/overview` | Return the current user, repository count, repository names, and active credential count. |
| `GET /api/v1/repositories` | List private repositories visible to the current session. |
| `GET /api/v1/repositories/{name}` | Return tag, digest, media type, size, and creation metadata. Nested names retain their `/` separator. |
| `GET /api/v1/auth/tokens` | List credential metadata without secrets. |
| `POST /api/v1/auth/tokens` | Create a scoped credential; the generated secret is returned once. |
| `POST /api/v1/auth/tokens/{id}/revoke` | Revoke a credential owned by the current user or by an administrator. |
| `GET /api/v1/audit` | Return recent security-sensitive events for the current session. |
| `GET /api/v1/webhooks` | List webhook endpoints without revealing secrets. |
| `POST /api/v1/webhooks` | Create an admin-owned endpoint; the HMAC secret is returned once. |
| `POST /api/v1/webhooks/{id}/disable` | Disable an endpoint without deleting delivery history. |
| `POST /api/v1/admin/gc` | Run or dry-run mark-and-sweep garbage collection as an administrator. |

Credential secrets are never returned by list endpoints. Action scopes are `pull`, `push`, `delete`, and `admin`; requested scopes are checked against the logged-in user’s credential before they reach the Docker Bearer token service.

The current dashboard covers sign-in, overview, repository browsing, one-time token reveal/revoke, password changes, and authenticator-app TOTP setup/disable. Audit and webhook controls are available through the documented control-plane API; they remain separate from the `/v2` protocol so browser cookies never authorize registry requests.
