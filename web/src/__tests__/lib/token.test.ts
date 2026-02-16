import { describe, it, expect } from "vitest";

import { generateToken, generateUserCode, signJwt, verifyJwt } from "../../lib/token";

describe("generateToken", () => {
  it("returns a UUID format string", () => {
    const token = generateToken();
    expect(token).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
  });

  it("returns unique tokens", () => {
    const tokens = new Set(Array.from({ length: 10 }, () => generateToken()));
    expect(tokens.size).toBe(10);
  });
});

describe("generateUserCode", () => {
  it("returns XXXX-XXXX format", () => {
    const code = generateUserCode();
    expect(code).toMatch(/^[A-Z2-9]{4}-[A-Z2-9]{4}$/);
  });

  it("does not contain ambiguous characters (0, 1, I, O)", () => {
    // Generate multiple codes to increase confidence
    for (let i = 0; i < 100; i++) {
      const code = generateUserCode();
      expect(code).not.toMatch(/[01IO]/);
    }
  });
});

describe("signJwt / verifyJwt", () => {
  const secret = "test-secret-key";

  it("signs and verifies a JWT", async () => {
    const payload = { userId: 42, iat: 1234567890 };
    const token = await signJwt(payload, secret);

    expect(typeof token).toBe("string");
    expect(token.split(".")).toHaveLength(3);

    const decoded = await verifyJwt(token, secret);
    expect(decoded).toEqual(payload);
  });

  it("returns null for invalid token", async () => {
    const result = await verifyJwt("invalid.token.here", secret);
    expect(result).toBeNull();
  });

  it("returns null for wrong secret", async () => {
    const payload = { userId: 1 };
    const token = await signJwt(payload, secret);
    const result = await verifyJwt(token, "wrong-secret");
    expect(result).toBeNull();
  });

  it("returns null for malformed token", async () => {
    expect(await verifyJwt("", secret)).toBeNull();
    expect(await verifyJwt("a.b", secret)).toBeNull();
    expect(await verifyJwt("a", secret)).toBeNull();
  });
});
