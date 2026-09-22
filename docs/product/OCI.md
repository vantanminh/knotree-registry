# OCI/Docker data plane

The registry protocol is rooted at `/v2/` and follows the OCI Distribution API shape. Protected requests receive a Bearer challenge with the token realm, service and repository scope. The token service intersects the requested scope with the PAT grant before issuing a short-lived JWT.

Implemented protocol paths:

- `GET /v2/` authenticated registry handshake.
- `GET`/`HEAD /v2/<name>/manifests/<reference>` for tags and `sha256:` digests.
- `GET`/`HEAD /v2/<name>/blobs/<digest>` with repository-level blob authorization.
- `GET /v2/<name>/tags/list` with `n`, `last`, and `Link` pagination.
- `POST /v2/<name>/blobs/uploads/`, `GET` upload status, ordered `PATCH`, final `PUT?digest=`, monolithic upload, and `DELETE` abort.
- cross-repository blob mount when the same Bearer token has pull access to the source and push access to the destination.

Manifest bytes are stored exactly as received. A parsed copy validates schema version, supported OCI/Docker media type, descriptor sizes and SHA-256 digests. The response includes `Docker-Distribution-Api-Version`, `Docker-Content-Digest`, `Content-Type`, `Content-Length`, `Location`, `Range`, and `Docker-Upload-UUID` where applicable. Errors use the OCI `{"errors":[...]}` envelope.

Upload finalization computes SHA-256 over the verified staging object and only then promotes it to the immutable `blobs/sha256/<prefix>/<digest>` key. A wrong digest deletes the staging object and cannot publish content. Local and memory stores support ordered append for tests; the R2 implementation buffers only a multipart tail, uploads fixed 16 MiB parts, completes the multipart object before verification, and uses the same promotion contract.
