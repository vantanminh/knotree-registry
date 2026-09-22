# Security model

The registry is private by default. R2 credentials are server-side only, the bucket remains private, and repository authorization is checked before content existence is revealed. Browser authentication will use secure HttpOnly SameSite cookies; Docker automation will use short-lived Bearer tokens minted from revocable, scoped credentials.

The storage abstraction deliberately separates an immutable object key from repository authorization. A content-addressed digest is not itself permission to read the bytes: later database relationships must prove that the requested repository is allowed to reference the object.

The current foundation validates configuration, rejects path traversal in the local store, atomically replaces local objects, adds request IDs to responses/log context, and avoids returning internal errors to clients. Authentication, authorization middleware, R2 and upload coordination are implemented in subsequent stories.
