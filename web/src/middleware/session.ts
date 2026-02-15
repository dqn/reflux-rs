import { createMiddleware } from "hono/factory";
import { getCookie, setCookie } from "hono/cookie";
import { drizzle } from "drizzle-orm/d1";
import { eq } from "drizzle-orm";

import type { Env } from "../lib/types";
import { verifyJwt, signJwt } from "../lib/token";
import { users } from "../db/schema";

interface SessionUser {
  id: number;
  email: string;
  username: string | null;
  apiToken: string | null;
  isPublic: boolean;
}

interface SessionEnv {
  Bindings: Env;
  Variables: {
    user: SessionUser;
  };
}

// Session authentication middleware using JWT cookies
export const sessionAuth = createMiddleware<SessionEnv>(async (c, next) => {
  const token = getCookie(c, "session");
  if (!token) {
    return c.redirect("/login");
  }

  const payload = await verifyJwt(token, c.env.JWT_SECRET);
  if (!payload || typeof payload.userId !== "number") {
    return c.redirect("/login");
  }

  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(users)
    .where(eq(users.id, payload.userId as number))
    .limit(1);

  const user = result[0];
  if (!user) {
    return c.redirect("/login");
  }

  c.set("user", user);
  await next();
});

// Optional session middleware that does not redirect on failure
export const optionalSession = createMiddleware<{
  Bindings: Env;
  Variables: {
    user: SessionUser | null;
  };
}>(async (c, next) => {
  const token = getCookie(c, "session");
  if (!token) {
    c.set("user", null);
    await next();
    return;
  }

  const payload = await verifyJwt(token, c.env.JWT_SECRET);
  if (!payload || typeof payload.userId !== "number") {
    c.set("user", null);
    await next();
    return;
  }

  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(users)
    .where(eq(users.id, payload.userId as number))
    .limit(1);

  c.set("user", result[0] ?? null);
  await next();
});

// Create a session cookie with JWT
export async function createSessionCookie(
  userId: number,
  secret: string,
): Promise<string> {
  return signJwt({ userId, iat: Math.floor(Date.now() / 1000) }, secret);
}

// Set session cookie on the response
export function setSessionCookie(
  c: { header: (name: string, value: string) => void },
  token: string,
): void {
  const cookie = `session=${token}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=604800`;
  c.header("Set-Cookie", cookie);
}
