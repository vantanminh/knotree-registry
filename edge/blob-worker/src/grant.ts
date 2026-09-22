export interface BlobGrant {
  key: string;
  digest: string;
  expiresAt: number;
  signature: string;
}

const encoder = new TextEncoder();

export function canonicalGrant(grant: Pick<BlobGrant, "key" | "digest" | "expiresAt">): string {
  return `BLOB\n${grant.key}\n${grant.digest}\n${grant.expiresAt}`;
}

export function expectedBlobKey(digest: string): string | null {
  if (!/^sha256:[0-9a-f]{64}$/.test(digest)) return null;
  const encoded = digest.slice("sha256:".length);
  return `blobs/sha256/${encoded.slice(0, 2)}/${encoded}`;
}

export async function signGrant(
  grant: Pick<BlobGrant, "key" | "digest" | "expiresAt">,
  secret: string,
): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign("HMAC", key, encoder.encode(canonicalGrant(grant)));
  return bytesToHex(new Uint8Array(signature));
}

export async function verifyGrant(grant: BlobGrant, secret: string, nowSeconds = Math.floor(Date.now() / 1000)): Promise<boolean> {
  if (grant.expiresAt < nowSeconds || expectedBlobKey(grant.digest) !== grant.key) return false;
  if (!/^[0-9a-f]{64}$/.test(grant.signature)) return false;
  const expected = await signGrant(grant, secret);
  const providedBytes = hexToBytes(grant.signature);
  const expectedBytes = hexToBytes(expected);
  return constantTimeEqual(providedBytes, expectedBytes);
}

export function parseGrant(url: URL): BlobGrant | null {
  const key = url.searchParams.get("key");
  const digest = url.searchParams.get("digest");
  const expiresAt = Number(url.searchParams.get("exp"));
  const signature = url.searchParams.get("sig");
  if (!key || !digest || !Number.isSafeInteger(expiresAt) || !signature) return null;
  return { key, digest, expiresAt, signature };
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function hexToBytes(value: string): Uint8Array {
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function constantTimeEqual(left: Uint8Array, right: Uint8Array): boolean {
  const subtle = crypto.subtle as SubtleCrypto & {
    timingSafeEqual?: (a: ArrayBuffer | ArrayBufferView, b: ArrayBuffer | ArrayBufferView) => boolean;
  };
  if (typeof subtle.timingSafeEqual === "function") {
    return subtle.timingSafeEqual.call(subtle, left, right);
  }
  let difference = left.length ^ right.length;
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    difference |= (left[index] ?? 0) ^ (right[index] ?? 0);
  }
  return difference === 0;
}
