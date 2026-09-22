import { describe, expect, it } from "vitest";
import { canonicalGrant, expectedBlobKey, signGrant, verifyGrant } from "../src/grant";

const digest = "sha256:" + "a".repeat(64);

describe("blob grants", () => {
  it("signs and verifies a digest-bound grant", async () => {
    const grant = { key: expectedBlobKey(digest)!, digest, expiresAt: 2_000_000_000 };
    const signature = await signGrant(grant, "test-secret");
    expect(canonicalGrant(grant)).toContain(grant.key);
    expect(await verifyGrant({ ...grant, signature }, "test-secret", 1_999_999_999)).toBe(true);
    expect(await verifyGrant({ ...grant, signature: "0".repeat(64) }, "test-secret", 1_999_999_999)).toBe(false);
  });

  it("rejects expiry and key substitution", async () => {
    const grant = { key: expectedBlobKey(digest)!, digest, expiresAt: 10 };
    const signature = await signGrant(grant, "test-secret");
    expect(await verifyGrant({ ...grant, signature }, "test-secret", 11)).toBe(false);
    expect(await verifyGrant({ ...grant, key: "blobs/sha256/ff/nope", signature }, "test-secret", 1)).toBe(false);
  });
});
