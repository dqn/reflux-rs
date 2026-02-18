import { Hono } from "hono";
import { eq, and, gt, asc } from "drizzle-orm";

import type { AppEnv } from "../lib/types";
import { isHigherLamp } from "../lib/lamp";
import { generateToken } from "../lib/token";
import { bearerAuth } from "../middleware/auth";
import { sessionAuth } from "../middleware/session";
import { cacheControl } from "../middleware/cache";
import { users, charts, lamps } from "../db/schema";
import { buildLampMap, groupChartsByTier } from "../lib/chart-table";
import { validateLampInput } from "../lib/validators";

interface LampInput {
  songId: number;
  difficulty: string;
  lamp: string;
  exScore?: number;
  missCount?: number;
}

interface BulkError {
  index: number;
  reason: string;
}

export const apiRoutes = new Hono<AppEnv>();

// GET /api/tables/:tableKey - Get chart entries + user lamps
apiRoutes.get(
  "/tables/:tableKey",
  cacheControl("public, max-age=60"),
  async (c) => {
    const tableKey = c.req.param("tableKey");
    const username = c.req.query("user");

    const db = c.get("db");

    // Get charts for this table
    const chartRows = await db
      .select()
      .from(charts)
      .where(eq(charts.tableKey, tableKey))
      .orderBy(asc(charts.sortOrder));

    if (chartRows.length === 0) {
      return c.json({ error: "Table not found" }, 404);
    }

    // If user is specified, get their lamps
    let lampMap = new Map<string, { lamp: string; exScore: number | null; missCount: number | null }>();

    if (username) {
      const userResult = await db
        .select()
        .from(users)
        .where(eq(users.username, username))
        .limit(1);

      const targetUser = userResult[0];
      if (targetUser) {
        if (!targetUser.isPublic) {
          return c.json({ error: "User profile is private" }, 403);
        }

        const userLamps = await db
          .select()
          .from(lamps)
          .where(eq(lamps.userId, targetUser.id));

        lampMap = buildLampMap(userLamps);
      }
    }

    const tiers = groupChartsByTier(chartRows, lampMap);
    return c.json({ tableKey, tiers });
  },
);

// POST /api/lamps - Update single lamp (Bearer auth)
apiRoutes.post("/lamps", bearerAuth, async (c) => {
  const body = await c.req.json<LampInput>();

  const validation = validateLampInput(body);
  if (!validation.valid) {
    return c.json({ error: validation.error }, 400);
  }

  const user = c.get("user");
  if (!user) {
    return c.json({ error: "Unauthorized" }, 401);
  }
  const db = c.get("db");

  // Check existing lamp
  const existing = await db
    .select()
    .from(lamps)
    .where(
      and(
        eq(lamps.userId, user.id),
        eq(lamps.songId, body.songId),
        eq(lamps.difficulty, body.difficulty),
      ),
    )
    .limit(1);

  const existingLamp = existing[0];
  const now = new Date().toISOString();

  if (existingLamp) {
    // Only update if new lamp is higher
    if (!isHigherLamp(body.lamp, existingLamp.lamp)) {
      // Still update ex_score and miss_count if provided
      const updates: Record<string, unknown> = { updatedAt: now };
      if (body.exScore !== undefined) {
        updates.exScore = body.exScore;
      }
      if (body.missCount !== undefined) {
        updates.missCount = body.missCount;
      }
      if (Object.keys(updates).length > 1) {
        await db
          .update(lamps)
          .set(updates)
          .where(eq(lamps.id, existingLamp.id));
      }
      return c.json({ updated: false, reason: "Current lamp is equal or higher" });
    }

    const updates: Record<string, unknown> = {
      lamp: body.lamp,
      updatedAt: now,
    };
    if (body.exScore !== undefined) {
      updates.exScore = body.exScore;
    }
    if (body.missCount !== undefined) {
      updates.missCount = body.missCount;
    }

    await db.update(lamps).set(updates).where(eq(lamps.id, existingLamp.id));
    return c.json({ updated: true });
  }

  // Insert new lamp
  await db.insert(lamps).values({
    userId: user.id,
    songId: body.songId,
    difficulty: body.difficulty,
    lamp: body.lamp,
    exScore: body.exScore ?? null,
    missCount: body.missCount ?? null,
    updatedAt: now,
  });

  return c.json({ updated: true });
});

// POST /api/lamps/bulk - Bulk update lamps (Bearer auth)
apiRoutes.post("/lamps/bulk", bearerAuth, async (c) => {
  // Handle gzip-compressed request bodies.
  // Cloudflare Workers may auto-decompress, but we handle it explicitly as fallback.
  let rawBody: unknown;
  const contentEncoding = c.req.header("Content-Encoding");
  if (contentEncoding === "gzip") {
    const compressed = await c.req.arrayBuffer();
    const ds = new DecompressionStream("gzip");
    const decompressed = new Response(new Blob([compressed]).stream().pipeThrough(ds));
    rawBody = await decompressed.json();
  } else {
    rawBody = await c.req.json();
  }

  // Bug-2 fix: Accept both { entries: [...] } and [...] formats
  let entries: LampInput[];
  if (Array.isArray(rawBody)) {
    entries = rawBody;
  } else if (rawBody && Array.isArray(rawBody.entries)) {
    entries = rawBody.entries;
  } else {
    return c.json({ error: "Expected an array of lamp entries or { entries: [...] }" }, 400);
  }

  const user = c.get("user");
  if (!user) {
    return c.json({ error: "Unauthorized" }, 401);
  }
  const db = c.get("db");
  const now = new Date().toISOString();
  let updatedCount = 0;
  let skippedCount = 0;
  const errors: BulkError[] = [];

  // Phase 3-2: Batch fetch existing lamps to avoid N+1
  const existingLamps = await db
    .select()
    .from(lamps)
    .where(eq(lamps.userId, user.id));

  const existingMap = new Map<string, typeof existingLamps[number]>();
  for (const l of existingLamps) {
    existingMap.set(`${l.songId}:${l.difficulty}`, l);
  }

  // Process entries and collect batch operations
  const inserts: Array<{
    userId: number;
    songId: number;
    difficulty: string;
    lamp: string;
    exScore: number | null;
    missCount: number | null;
    updatedAt: string;
  }> = [];

  const updates: Array<{
    id: number;
    values: Record<string, unknown>;
  }> = [];

  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i]!;

    const validation = validateLampInput(entry);
    if (!validation.valid) {
      errors.push({ index: i, reason: validation.error! });
      skippedCount++;
      continue;
    }

    const key = `${entry.songId}:${entry.difficulty}`;
    const existingLamp = existingMap.get(key);

    if (existingLamp) {
      if (isHigherLamp(entry.lamp, existingLamp.lamp)) {
        const updateValues: Record<string, unknown> = {
          lamp: entry.lamp,
          updatedAt: now,
        };
        if (entry.exScore !== undefined) {
          updateValues.exScore = entry.exScore;
        }
        if (entry.missCount !== undefined) {
          updateValues.missCount = entry.missCount;
        }
        updates.push({ id: existingLamp.id, values: updateValues });
        updatedCount++;
      } else {
        skippedCount++;
      }
    } else {
      inserts.push({
        userId: user.id,
        songId: entry.songId,
        difficulty: entry.difficulty,
        lamp: entry.lamp,
        exScore: entry.exScore ?? null,
        missCount: entry.missCount ?? null,
        updatedAt: now,
      });
      updatedCount++;
    }
  }

  // Execute batch operations using D1 batch API
  const statements: D1PreparedStatement[] = [];

  for (const ins of inserts) {
    statements.push(
      c.env.DB.prepare(
        "INSERT INTO lamps (user_id, song_id, difficulty, lamp, ex_score, miss_count, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
      ).bind(
        ins.userId,
        ins.songId,
        ins.difficulty,
        ins.lamp,
        ins.exScore,
        ins.missCount,
        ins.updatedAt,
      ),
    );
  }

  for (const upd of updates) {
    const setClauses: string[] = [];
    const bindValues: unknown[] = [];
    for (const [k, v] of Object.entries(upd.values)) {
      // Convert camelCase to snake_case for SQL
      const col = k.replace(/[A-Z]/g, (m) => `_${m.toLowerCase()}`);
      setClauses.push(`${col} = ?`);
      bindValues.push(v);
    }
    bindValues.push(upd.id);
    statements.push(
      c.env.DB.prepare(
        `UPDATE lamps SET ${setClauses.join(", ")} WHERE id = ?`,
      ).bind(...bindValues),
    );
  }

  if (statements.length > 0) {
    await c.env.DB.batch(statements);
  }

  return c.json({
    updated: updatedCount,
    skipped: skippedCount,
    total: entries.length,
    errors: errors.length > 0 ? errors : undefined,
  });
});

// GET /api/lamps/updated-since - Polling endpoint
apiRoutes.get(
  "/lamps/updated-since",
  cacheControl("no-cache"),
  async (c) => {
    const since = c.req.query("since");
    const username = c.req.query("user");

    if (!since || !username) {
      return c.json({ error: "since and user query params are required" }, 400);
    }

    const db = c.get("db");
    const userResult = await db
      .select()
      .from(users)
      .where(eq(users.username, username))
      .limit(1);

    const targetUser = userResult[0];
    if (!targetUser) {
      return c.json({ lamps: [] });
    }

    const updatedLamps = await db
      .select()
      .from(lamps)
      .where(
        and(
          eq(lamps.userId, targetUser.id),
          gt(lamps.updatedAt, since),
        ),
      );

    return c.json({
      lamps: updatedLamps.map((l) => ({
        songId: l.songId,
        difficulty: l.difficulty,
        lamp: l.lamp,
        exScore: l.exScore,
        missCount: l.missCount,
        updatedAt: l.updatedAt,
      })),
    });
  },
);

// POST /api/charts/sync - Sync title-mapping.json to charts table (Admin auth)
apiRoutes.post("/charts/sync", async (c) => {
  const adminToken = c.req.header("Authorization")?.replace("Bearer ", "");
  if (adminToken !== c.env.ADMIN_TOKEN) {
    return c.json({ error: "Unauthorized" }, 401);
  }

  const body = await c.req.json<
    Record<
      string,
      Array<{
        songId: number;
        title: string;
        difficulty: string;
        tier: string;
        attributes?: string;
        sortOrder?: number;
      }>
    >
  >();

  const db = c.get("db");
  let upsertCount = 0;

  for (const [tableKey, tableEntries] of Object.entries(body)) {
    for (const entry of tableEntries) {
      // Try to update existing, insert if not found
      const existing = await db
        .select()
        .from(charts)
        .where(
          and(
            eq(charts.tableKey, tableKey),
            eq(charts.songId, entry.songId),
            eq(charts.difficulty, entry.difficulty),
          ),
        )
        .limit(1);

      if (existing[0]) {
        await db
          .update(charts)
          .set({
            songId: entry.songId,
            title: entry.title,
            difficulty: entry.difficulty,
            tier: entry.tier,
            attributes: entry.attributes ?? null,
            sortOrder: entry.sortOrder ?? null,
          })
          .where(eq(charts.id, existing[0].id));
      } else {
        await db.insert(charts).values({
          tableKey,
          songId: entry.songId,
          title: entry.title,
          difficulty: entry.difficulty,
          tier: entry.tier,
          attributes: entry.attributes ?? null,
          sortOrder: entry.sortOrder ?? null,
        });
      }
      upsertCount++;
    }
  }

  return c.json({ synced: upsertCount });
});

// PATCH /api/users/me - Update user settings (session auth)
apiRoutes.patch("/users/me", sessionAuth, async (c) => {
  const body = await c.req.json<{ isPublic?: boolean }>();
  const user = c.get("user");
  if (!user) {
    return c.json({ error: "Unauthorized" }, 401);
  }
  const db = c.get("db");

  const updates: Record<string, unknown> = {};
  if (body.isPublic !== undefined) {
    updates.isPublic = body.isPublic;
  }

  if (Object.keys(updates).length > 0) {
    await db.update(users).set(updates).where(eq(users.id, user.id));
  }

  return c.json({ ok: true });
});

// GET /api/users/me/token - Get API token (session auth)
apiRoutes.get("/users/me/token", sessionAuth, async (c) => {
  const user = c.get("user");
  if (!user) {
    return c.json({ error: "Unauthorized" }, 401);
  }
  return c.json({ apiToken: user.apiToken });
});

// POST /api/users/me/token/regenerate - Regenerate API token (session auth)
apiRoutes.post("/users/me/token/regenerate", sessionAuth, async (c) => {
  const user = c.get("user");
  if (!user) {
    return c.json({ error: "Unauthorized" }, 401);
  }
  const newToken = generateToken();
  const now = new Date().toISOString();
  const db = c.get("db");

  await db
    .update(users)
    .set({ apiToken: newToken, apiTokenCreatedAt: now })
    .where(eq(users.id, user.id));

  return c.json({ apiToken: newToken });
});
