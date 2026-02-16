import { Hono } from "hono";
import { drizzle } from "drizzle-orm/d1";
import { eq } from "drizzle-orm";

import type { Env } from "../lib/types";
import { generateToken, generateUserCode } from "../lib/token";
import { hashPassword, verifyPassword } from "../lib/password";
import { users, deviceCodes } from "../db/schema";
import {
  sessionAuth,
  createSessionCookie,
  setSessionCookie,
} from "../middleware/session";
import { DevicePage } from "../components/DevicePage";

const RESERVED_USERNAMES = ["login", "register", "settings", "auth", "api", "admin", "guide"];

export const authRoutes = new Hono<{
  Bindings: Env;
  Variables: { user: { id: number; email: string; username: string; apiToken: string | null; isPublic: boolean } };
}>();

// POST /auth/login - Email + password login
authRoutes.post("/login", async (c) => {
  const body = await c.req.parseBody();
  const email = body["email"];
  const password = body["password"];

  if (typeof email !== "string" || typeof password !== "string") {
    const { LoginPage } = await import("../components/LoginPage");
    return c.html(<LoginPage error="Email and password are required" />);
  }

  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(users)
    .where(eq(users.email, email))
    .limit(1);

  const user = result[0];

  if (!user) {
    // Timing attack mitigation: compute a dummy hash
    await hashPassword(password);
    const { LoginPage } = await import("../components/LoginPage");
    return c.html(<LoginPage error="Invalid email or password" values={{ email }} />);
  }

  const valid = await verifyPassword(password, user.passwordHash);
  if (!valid) {
    const { LoginPage } = await import("../components/LoginPage");
    return c.html(<LoginPage error="Invalid email or password" values={{ email }} />);
  }

  const sessionToken = await createSessionCookie(user.id, c.env.JWT_SECRET);
  setSessionCookie(c, sessionToken);

  return c.redirect("/");
});

// POST /auth/register - Email + password + username registration
authRoutes.post("/register", async (c) => {
  const body = await c.req.parseBody();
  const email = body["email"];
  const password = body["password"];
  const username = body["username"];

  if (
    typeof email !== "string" ||
    typeof password !== "string" ||
    typeof username !== "string"
  ) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(<RegisterPage error="All fields are required" />);
  }

  const values = { email, username };

  // Validate password
  if (password.length < 8 || password.length > 72) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(
      <RegisterPage error="Password must be 8-72 characters" values={values} />,
    );
  }

  // Validate username
  const trimmed = username.trim().toLowerCase();
  if (!/^[a-z0-9_-]{3,20}$/.test(trimmed)) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(
      <RegisterPage
        error="Username must be 3-20 characters (a-z, 0-9, -, _)"
        values={values}
      />,
    );
  }

  if (RESERVED_USERNAMES.includes(trimmed)) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(
      <RegisterPage error="This username is not available" values={values} />,
    );
  }

  const db = drizzle(c.env.DB);

  // Check email uniqueness
  const existingEmail = await db
    .select()
    .from(users)
    .where(eq(users.email, email))
    .limit(1);

  if (existingEmail.length > 0) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(
      <RegisterPage error="This email is already registered" values={values} />,
    );
  }

  // Check username uniqueness
  const existingUsername = await db
    .select()
    .from(users)
    .where(eq(users.username, trimmed))
    .limit(1);

  if (existingUsername.length > 0) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(
      <RegisterPage error="Username already taken" values={values} />,
    );
  }

  const passwordHash = await hashPassword(password);
  const apiToken = generateToken();

  const inserted = await db
    .insert(users)
    .values({
      email,
      username: trimmed,
      passwordHash,
      apiToken,
    })
    .returning();

  const user = inserted[0];
  if (!user) {
    const { RegisterPage } = await import("../components/RegisterPage");
    return c.html(
      <RegisterPage error="Failed to create account" values={values} />,
    );
  }

  // Auto-login after registration
  const sessionToken = await createSessionCookie(user.id, c.env.JWT_SECRET);
  setSessionCookie(c, sessionToken);

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
