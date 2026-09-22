# Authentication and authorization

The service has two authentication surfaces:

1. Browser users authenticate with `POST /api/v1/auth/login`. A successful login rotates a random session token and returns it only in an HttpOnly, SameSite=Strict cookie. Production deployments keep `COOKIE_SECURE=true` so browsers send the cookie only over TLS.
2. Docker/OCI clients authenticate with a one-time-revealed PAT or robot credential. The client sends Basic credentials to `/auth/token`; the service intersects the requested `repository:<name>:pull,push,delete` scope with the credential's stored grants and returns a short-lived ES256 Bearer JWT.

PAT secrets are never stored in plaintext. Only a salted verifier is retained in the auth service boundary; revocation blocks future token minting immediately. Already-issued registry JWTs are intentionally bounded by `REGISTRY_TOKEN_TTL_SECONDS` and are checked for issuer, audience, signature, expiry and repository action on every protected request.

For local development, set `BOOTSTRAP_ADMIN_USERNAME`, `BOOTSTRAP_ADMIN_PASSWORD` (at least 12 characters), and `COOKIE_SECURE=false`. A fresh process can then log in, create a scoped credential through `/api/v1/auth/tokens`, and use that credential with Docker's standard Basic-to-Bearer exchange. Bootstrap values must be provided through the runtime environment and must not be committed.
