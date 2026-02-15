import { Hono } from "hono";
import { drizzle } from "drizzle-orm/d1";
import { eq, and } from "drizzle-orm";

import type { Env } from "../lib/types";
import { generateToken, generateUserCode } from "../lib/token";
import { sendMagicLinkEmail } from "../lib/email";
import { users, magicLinks, deviceCodes } from "../db/schema";
import {
  sessionAuth,
  createSessionCookie,
  setSessionCookie,
} from "../middleware/session";
import { RegisterPage } from "../components/RegisterPage";
import { DevicePage } from "../components/DevicePage";

export const authRoutes = new Hono<{
  Bindings: Env;
  Variables: { user: { id: number; email: string; username: string | null; apiToken: string | null; isPublic: boolean } };
}>();

// POST /auth/login - Send magic link email
authRoutes.post("/login", async (c) => {
  const body = await c.req.json<{ email?: string }>();
  if (!body.email) {
    return c.json({ error: "Email is required" }, 400);
  }

  const token = generateToken();
  const expiresAt = new Date(Date.now() + 15 * 60 * 1000).toISOString();

  const db = drizzle(c.env.DB);
  await db.insert(magicLinks).values({
    email: body.email,
    token,
    expiresAt,
  });

  const magicLinkUrl = `${c.env.APP_URL}/auth/verify?token=${token}`;
  await sendMagicLinkEmail(body.email, magicLinkUrl, c.env.RESEND_API_KEY);

  return c.json({ ok: true });
});

// GET /auth/verify - Verify magic link token
authRoutes.get("/verify", async (c) => {
  const token = c.req.query("token");
  if (!token) {
    return c.text("Invalid link", 400);
  }

  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(magicLinks)
    .where(eq(magicLinks.token, token))
    .limit(1);

  const link = result[0];
  if (!link) {
    return c.text("Invalid or expired link", 400);
  }

  if (link.usedAt) {
    return c.text("Link already used", 400);
  }

  if (new Date(link.expiresAt) < new Date()) {
    return c.text("Link expired", 400);
  }

  // Mark as used
  await db
    .update(magicLinks)
    .set({ usedAt: new Date().toISOString() })
    .where(eq(magicLinks.id, link.id));

  // Find or create user
  const existingUsers = await db
    .select()
    .from(users)
    .where(eq(users.email, link.email))
    .limit(1);

  let user = existingUsers[0];

  if (!user) {
    const apiToken = generateToken();
    const inserted = await db
      .insert(users)
      .values({
        email: link.email,
        apiToken,
      })
      .returning();
    user = inserted[0];
  }

  if (!user) {
    return c.text("Failed to create user", 500);
  }

  // Issue session cookie
  const sessionToken = await createSessionCookie(user.id, c.env.JWT_SECRET);
  setSessionCookie(c, sessionToken);

  // Redirect to username setup if needed, otherwise home
  if (!user.username) {
    return c.redirect("/auth/register");
  }

  return c.redirect("/");
});

// GET /auth/register - Username registration page
authRoutes.get("/register", sessionAuth, (c) => {
  return c.html(<RegisterPage />);
});

// POST /auth/register - Handle username registration
authRoutes.post("/register", sessionAuth, async (c) => {
  const body = await c.req.parseBody();
  const username = body["username"];
  if (typeof username !== "string" || !username.trim()) {
    return c.html(<RegisterPage error="Username is required" />);
  }

  const trimmed = username.trim().toLowerCase();
  if (!/^[a-z0-9_-]{3,20}$/.test(trimmed)) {
    return c.html(
      <RegisterPage error="Username must be 3-20 characters (a-z, 0-9, -, _)" />,
    );
  }

  // Reserved paths
  const reserved = ["login", "settings", "auth", "api", "admin"];
  if (reserved.includes(trimmed)) {
    return c.html(<RegisterPage error="This username is not available" />);
  }

  const db = drizzle(c.env.DB);

  // Check uniqueness
  const existing = await db
    .select()
    .from(users)
    .where(eq(users.username, trimmed))
    .limit(1);

  if (existing.length > 0) {
    return c.html(<RegisterPage error="Username already taken" />);
  }

  const user = c.get("user");
  await db.update(users).set({ username: trimmed }).where(eq(users.id, user.id));

  return c.redirect("/");
});

// POST /auth/device/code - Generate device code + user code
authRoutes.post("/device/code", async (c) => {
  const deviceCode = generateToken();
  const userCode = generateUserCode();
  const expiresAt = new Date(Date.now() + 5 * 60 * 1000).toISOString();

  const db = drizzle(c.env.DB);
  await db.insert(deviceCodes).values({
    deviceCode,
    userCode,
    expiresAt,
  });

  return c.json({
    device_code: deviceCode,
    user_code: userCode,
    expires_in: 300,
    interval: 5,
    verification_uri: `${c.env.APP_URL}/auth/device`,
  });
});

// POST /auth/device/token - Poll for device authorization
authRoutes.post("/device/token", async (c) => {
  const body = await c.req.json<{ device_code?: string }>();
  if (!body.device_code) {
    return c.json({ error: "device_code is required" }, 400);
  }

  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(deviceCodes)
    .where(eq(deviceCodes.deviceCode, body.device_code))
    .limit(1);

  const code = result[0];
  if (!code) {
    return c.json({ error: "invalid_device_code" }, 400);
  }

  if (new Date(code.expiresAt) < new Date()) {
    return c.json({ error: "expired_token" }, 400);
  }

  if (code.apiToken) {
    return c.json({ access_token: code.apiToken, token_type: "Bearer" });
  }

  return c.json({ error: "authorization_pending" }, 428);
});

// GET /auth/device - Device confirmation page (requires session)
authRoutes.get("/device", sessionAuth, async (c) => {
  const userCode = c.req.query("code");
  return c.html(<DevicePage userCode={userCode ?? ""} />);
});

// POST /auth/device/confirm - Confirm device authorization
authRoutes.post("/device/confirm", sessionAuth, async (c) => {
  const body = await c.req.parseBody();
  const userCode = body["user_code"];
  if (typeof userCode !== "string" || !userCode.trim()) {
    return c.html(<DevicePage userCode="" error="User code is required" />);
  }

  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(deviceCodes)
    .where(eq(deviceCodes.userCode, userCode.trim().toUpperCase()))
    .limit(1);

  const code = result[0];
  if (!code) {
    return c.html(
      <DevicePage userCode={userCode} error="Invalid code" />,
    );
  }

  if (new Date(code.expiresAt) < new Date()) {
    return c.html(
      <DevicePage userCode={userCode} error="Code expired" />,
    );
  }

  if (code.apiToken) {
    return c.html(
      <DevicePage userCode={userCode} error="Code already used" />,
    );
  }

  const user = c.get("user");

  // Ensure user has an API token
  let apiToken = user.apiToken;
  if (!apiToken) {
    apiToken = generateToken();
    await db
      .update(users)
      .set({ apiToken })
      .where(eq(users.id, user.id));
  }

  // Link device code to user
  await db
    .update(deviceCodes)
    .set({ userId: user.id, apiToken })
    .where(eq(deviceCodes.deviceCode, code.deviceCode));

  return c.html(
    <DevicePage userCode={userCode} success={true} />,
  );
});

// POST /auth/logout
authRoutes.post("/logout", (c) => {
  c.header(
    "Set-Cookie",
    "session=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0",
  );
  return c.redirect("/login");
});
