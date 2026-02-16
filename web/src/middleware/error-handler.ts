import type { Context } from "hono";
import type { AppEnv } from "../lib/types";

export function setupErrorHandler(app: {
  onError: (handler: (err: Error, c: Context<AppEnv>) => Response | Promise<Response>) => void;
}): void {
  app.onError((err, c) => {
    console.error(
      JSON.stringify({
        error: err.message,
        stack: err.stack,
        path: c.req.path,
        method: c.req.method,
      }),
    );

    const isApi =
      c.req.path.startsWith("/api/") || c.req.path.startsWith("/auth/device/code") || c.req.path.startsWith("/auth/device/token");

    if (isApi) {
      return c.json({ error: "Internal server error" }, 500);
    }

    return c.html(
      `<!DOCTYPE html>
<html>
<head><title>Error</title></head>
<body style="background:#111;color:#e0e0e0;font-family:system-ui;display:flex;justify-content:center;align-items:center;min-height:100vh;margin:0;">
  <div style="text-align:center;">
    <h1 style="font-weight:300;">Something went wrong</h1>
    <p style="color:#999;">Please try again later.</p>
    <a href="/" style="color:#3db8c9;">Go home</a>
  </div>
</body>
</html>`,
      500,
    );
  });
}
