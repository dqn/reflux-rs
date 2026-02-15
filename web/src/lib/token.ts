// Generate a random API token
export function generateToken(): string {
  return crypto.randomUUID();
}

// Generate an 8-character user code in XXXX-XXXX format
export function generateUserCode(): string {
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  const code = Array.from(bytes, (b) => chars[b % chars.length]).join("");
  return `${code.slice(0, 4)}-${code.slice(4)}`;
}

// Encode bytes to base64url
function base64urlEncode(data: Uint8Array): string {
  const binString = Array.from(data, (b) => String.fromCharCode(b)).join("");
  return btoa(binString).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

// Decode base64url to bytes
function base64urlDecode(str: string): Uint8Array {
  const padded = str.replace(/-/g, "+").replace(/_/g, "/");
  const binString = atob(padded);
  return Uint8Array.from(binString, (c) => c.charCodeAt(0));
}

// Encode string to base64url
function encodePayload(obj: Record<string, unknown>): string {
  const json = JSON.stringify(obj);
  return base64urlEncode(new TextEncoder().encode(json));
}

// Import HMAC key for JWT signing/verification
async function importKey(secret: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  );
}

// Sign a JWT with HMAC-SHA256
export async function signJwt(
  payload: Record<string, unknown>,
  secret: string,
): Promise<string> {
  const header = encodePayload({ alg: "HS256", typ: "JWT" });
  const body = encodePayload(payload);
  const message = `${header}.${body}`;

  const key = await importKey(secret);
  const signature = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(message),
  );

  return `${message}.${base64urlEncode(new Uint8Array(signature))}`;
}

// Verify a JWT and return the payload
export async function verifyJwt(
  token: string,
  secret: string,
): Promise<Record<string, unknown> | null> {
  const parts = token.split(".");
  if (parts.length !== 3) {
    return null;
  }

  const [header, body, signature] = parts;
  if (!header || !body || !signature) {
    return null;
  }

  const key = await importKey(secret);
  const message = `${header}.${body}`;
  const signatureBytes = base64urlDecode(signature);

  const valid = await crypto.subtle.verify(
    "HMAC",
    key,
    signatureBytes,
    new TextEncoder().encode(message),
  );

  if (!valid) {
    return null;
  }

  const payloadJson = new TextDecoder().decode(base64urlDecode(body));
  return JSON.parse(payloadJson) as Record<string, unknown>;
}
