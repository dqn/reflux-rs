import { createMiddleware } from "hono/factory";

import type { AppEnv } from "../lib/types";

interface RateLimitOptions {
  max: number;
  windowSeconds: number;
}

export function rateLimit(options: RateLimitOptions) {
  return createMiddleware<AppEnv>(async (c, next) => {
    const ip = c.req.header("cf-connecting-ip") ?? "unknown";
    const path = new URL(c.req.url).pathname;
    const key = `${ip}:${path}`;
    const now = Date.now();
    const windowStart = new Date(now - options.windowSeconds * 1000).toISOString();

    const d1 = c.env.DB;

    // Clean expired entries
    await d1
      .prepare("DELETE FROM rate_limits WHERE created_at < ?")
      .bind(windowStart)
      .run()
      .catch(() => {});

    // Count recent requests
    const countResult = await d1
      .prepare(
        "SELECT COUNT(*) as count FROM rate_limits WHERE key = ? AND created_at >= ?",
      )
      .bind(key, windowStart)
      .first<{ count: number }>();

    const count = countResult?.count ?? 0;

    // Set rate limit headers
    const remaining = Math.max(0, options.max - count);
    c.header("X-RateLimit-Limit", String(options.max));
    c.header("X-RateLimit-Remaining", String(remaining));
    c.header(
      "X-RateLimit-Reset",
      String(Math.ceil((now + options.windowSeconds * 1000) / 1000)),
    );

    if (count >= options.max) {
      return c.json(
        { error: "Too many requests. Please try again later." },
        429,
      );
    }

    // Record this request
    await d1
      .prepare("INSERT INTO rate_limits (key, created_at) VALUES (?, ?)")
      .bind(key, new Date(now).toISOString())
      .run()
      .catch(() => {});

    await next();
  });
}
