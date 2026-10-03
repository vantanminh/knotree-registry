# Authentication and authorization

The service has two authentication surfaces:

1. Browser users sign in with their Knotree account through `/api/v1/auth/sso/start` (Knotree Accounts, OIDC with S256 PKCE). The callback issues a random session token only in an HttpOnly, SameSite=Strict cookie. Production deployments keep `COOKIE_SECURE=true` so browsers send the cookie only over TLS. There is no Registry password; two-step verification is enforced by the Knotree account.
2. Docker/OCI clients authenticate with a one-time-revealed PAT or robot credential. The client sends Basic credentials to `/auth/token`; the service intersects the requested `repository:<name>:pull,push,delete` scope with the credential's stored grants and returns a short-lived ES256 Bearer JWT.

PAT secrets are never stored in plaintext. Only a salted verifier is retained in the auth service boundary; revocation blocks future token minting immediately. Already-issued registry JWTs are intentionally bounded by `REGISTRY_TOKEN_TTL_SECONDS` and are checked for issuer, audience, signature, expiry and repository action on every protected request.

Passwords, emails and two-step verification are managed in the Knotree account. Administrators are the Knotree account ids in `SSO_ADMIN_SUBJECTS`.

For local development, run Knotree Accounts locally (or point `SSO_ISSUER` at a development issuer) with `COOKIE_SECURE=false`. After signing in, create a scoped credential through `/api/v1/auth/tokens` and use it with Docker's standard Basic-to-Bearer exchange.
