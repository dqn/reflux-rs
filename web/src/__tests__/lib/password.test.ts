import { describe, it, expect } from "vitest";

import { hashPassword, verifyPassword } from "../../lib/password";

describe("hashPassword", () => {
  it("returns a pbkdf2-prefixed hash", async () => {
    const hash = await hashPassword("test-password");
    expect(hash).toMatch(/^pbkdf2:\d+:.+:.+$/);
  });

  it("generates unique hashes for same password (different salts)", async () => {
    const hash1 = await hashPassword("same-password");
    const hash2 = await hashPassword("same-password");
    expect(hash1).not.toBe(hash2);
  });

  it("uses 600000 iterations", async () => {
    const hash = await hashPassword("test");
    const iterations = hash.split(":")[1];
    expect(iterations).toBe("600000");
  });
});

describe("verifyPassword", () => {
  it("verifies correct password", async () => {
    const hash = await hashPassword("my-secret");
    expect(await verifyPassword("my-secret", hash)).toBe(true);
  });

  it("rejects wrong password", async () => {
    const hash = await hashPassword("correct");
    expect(await verifyPassword("wrong", hash)).toBe(false);
  });

  it("returns false for invalid stored format", async () => {
    expect(await verifyPassword("test", "invalid")).toBe(false);
    expect(await verifyPassword("test", "")).toBe(false);
    expect(await verifyPassword("test", "sha256:100:abc:def")).toBe(false);
  });
});
