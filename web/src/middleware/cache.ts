import { createMiddleware } from "hono/factory";

import type { AppEnv } from "../lib/types";

export function cacheControl(value: string) {
  return createMiddleware<AppEnv>(async (c, next) => {
    await next();
    c.header("Cache-Control", value);
  });
}
