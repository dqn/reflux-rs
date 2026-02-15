const ITERATIONS = 600_000;
const HASH_LENGTH = 32;
const SALT_LENGTH = 16;

export async function hashPassword(password: string): Promise<string> {
  const encoder = new TextEncoder();
  const salt = crypto.getRandomValues(new Uint8Array(SALT_LENGTH));

  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(password),
    "PBKDF2",
    false,
    ["deriveBits"],
  );

  const hash = await crypto.subtle.deriveBits(
    { name: "PBKDF2", salt, iterations: ITERATIONS, hash: "SHA-256" },
    key,
    HASH_LENGTH * 8,
  );

  const saltBase64 = btoa(String.fromCharCode(...salt));
  const hashBase64 = btoa(String.fromCharCode(...new Uint8Array(hash)));

  return `pbkdf2:${ITERATIONS}:${saltBase64}:${hashBase64}`;
}

export async function verifyPassword(
  password: string,
  stored: string,
): Promise<boolean> {
  const [prefix, iterStr, saltStr, hashStr] = stored.split(":");
  if (!prefix || !iterStr || !saltStr || !hashStr || prefix !== "pbkdf2") {
    return false;
  }

  const iterations = Number(iterStr);
  const salt = Uint8Array.from(atob(saltStr), (c) => c.charCodeAt(0));
  const expectedHash = Uint8Array.from(atob(hashStr), (c) => c.charCodeAt(0));

  const encoder = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(password),
    "PBKDF2",
    false,
    ["deriveBits"],
  );

  const actualHash = new Uint8Array(
    await crypto.subtle.deriveBits(
      { name: "PBKDF2", salt, iterations, hash: "SHA-256" },
      key,
      expectedHash.length * 8,
    ),
  );

  if (actualHash.length !== expectedHash.length) {
    return false;
  }

  // Constant-time comparison: XOR all bytes and check result
  // Timing leakage is negligible here since PBKDF2 dominates execution time
  let diff = 0;
  for (let i = 0; i < actualHash.length; i++) {
    diff |= actualHash[i]! ^ expectedHash[i]!;
  }
  return diff === 0;
}
