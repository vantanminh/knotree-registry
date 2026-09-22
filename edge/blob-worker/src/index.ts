import { parseGrant, verifyGrant } from "./grant";

type BlobWorkerEnv = Env & { DOWNLOAD_GRANT_SECRET: string };

export default {
  async fetch(request, env): Promise<Response> {
    try {
      const url = new URL(request.url);
      if (url.pathname !== "/v1/blob") return jsonError("not found", 404);
      if (request.method !== "GET" && request.method !== "HEAD") {
        return new Response(null, { status: 405, headers: { allow: "GET, HEAD" } });
      }
      const grant = parseGrant(url);
      if (!grant || !(await verifyGrant(grant, env.DOWNLOAD_GRANT_SECRET))) {
        return jsonError("expired or invalid grant", 401);
      }

      const total = await env.BLOB_BUCKET.head(grant.key);
      if (!total) return jsonError("blob not found", 404);
      const range = parseRange(request.headers.get("range"), total.size);
      if (range === "invalid") {
        return new Response(null, {
          status: 416,
          headers: { "content-range": `bytes */${total.size}`, "accept-ranges": "bytes" },
        });
      }

      let object: R2Object = total;
      let body: ReadableStream | null = null;
      if (request.method !== "HEAD") {
        const fetched = range
          ? await env.BLOB_BUCKET.get(grant.key, { range })
          : await env.BLOB_BUCKET.get(grant.key);
        if (!fetched) return jsonError("blob not found", 404);
        if (!("body" in fetched)) return jsonError("blob body unavailable", 502);
        object = fetched;
        body = fetched.body;
      }

      const headers = new Headers();
      object.writeHttpMetadata(headers);
      headers.set("content-type", "application/octet-stream");
      headers.set("etag", object.httpEtag);
      headers.set("docker-content-digest", grant.digest);
      headers.set("accept-ranges", "bytes");
      headers.set("cache-control", "public, max-age=31536000, immutable");
      if (range) {
        const end = range.offset + range.length - 1;
        headers.set("content-range", `bytes ${range.offset}-${end}/${total.size}`);
        headers.set("content-length", String(range.length));
      } else {
        headers.set("content-length", String(total.size));
      }
      return new Response(body, {
        status: range ? 206 : 200,
        headers,
      });
    } catch (error) {
      console.error(JSON.stringify({ message: "blob worker request failed", error: error instanceof Error ? error.message : String(error) }));
      return jsonError("internal error", 500);
    }
  },
} satisfies ExportedHandler<BlobWorkerEnv>;

type ParsedRange = { offset: number; length: number };

function parseRange(value: string | null, size: number): ParsedRange | null | "invalid" {
  if (!value) return null;
  const match = /^bytes=(\d*)-(\d*)$/.exec(value);
  if (!match) return "invalid";
  const start = match[1] ? Number(match[1]) : null;
  const end = match[2] ? Number(match[2]) : null;
  if (start === null && end === null) return "invalid";
  if (start !== null && (!Number.isSafeInteger(start) || start >= size)) return "invalid";
  if (end !== null && (!Number.isSafeInteger(end) || end < 0)) return "invalid";
  if (start === null) {
    const length = Math.min(end as number, size);
    return length > 0 ? { offset: size - length, length } : "invalid";
  }
  const last = Math.min(end ?? size - 1, size - 1);
  return last >= start ? { offset: start, length: last - start + 1 } : "invalid";
}

function jsonError(message: string, status: number): Response {
  return Response.json({ error: message }, { status, headers: { "cache-control": "no-store" } });
}
