import { Hono } from "hono";

import type { Env } from "./lib/types";
import { authRoutes } from "./routes/auth";
import { apiRoutes } from "./routes/api";
import { pageRoutes } from "./routes/pages";

const app = new Hono<{ Bindings: Env }>();

app.route("/auth", authRoutes);
app.route("/api", apiRoutes);
app.route("/", pageRoutes);

export default app;
