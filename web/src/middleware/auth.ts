import { createMiddleware } from "hono/factory";
import { drizzle } from "drizzle-orm/d1";
import { eq } from "drizzle-orm";

import type { Env } from "../lib/types";
import { users } from "../db/schema";

interface AuthUser {
  id: number;
  email: string;
  username: string | null;
  apiToken: string | null;
  isPublic: boolean;
}

interface AuthEnv {
  Bindings: Env;
  Variables: {
    user: AuthUser;
  };
}

// Bearer token authentication middleware for API routes
export const bearerAuth = createMiddleware<AuthEnv>(async (c, next) => {
  const authorization = c.req.header("Authorization");
  if (!authorization) {
    return c.json({ error: "Missing Authorization header" }, 401);
  }

  const match = authorization.match(/^Bearer\s+(.+)$/);
  if (!match?.[1]) {
    return c.json({ error: "Invalid Authorization header format" }, 401);
  }

  const token = match[1];
  const db = drizzle(c.env.DB);
  const result = await db
    .select()
    .from(users)
    .where(eq(users.apiToken, token))
    .limit(1);

  const user = result[0];
  if (!user) {
    return c.json({ error: "Invalid API token" }, 401);
  }

  c.set("user", user);
  await next();
});
