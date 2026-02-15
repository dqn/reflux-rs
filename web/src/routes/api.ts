import { Hono } from "hono";
import { drizzle } from "drizzle-orm/d1";
import { eq, and, gt, sql } from "drizzle-orm";

import type { Env } from "../lib/types";
import { isHigherLamp, isValidLamp } from "../lib/lamp";
import { generateToken } from "../lib/token";
import { bearerAuth } from "../middleware/auth";
import { sessionAuth } from "../middleware/session";
import { users, charts, lamps } from "../db/schema";

interface LampInput {
  infinitasTitle: string;
  difficulty: string;
  lamp: string;
  exScore?: number;
  missCount?: number;
}

export const apiRoutes = new Hono<{
  Bindings: Env;
  Variables: {
    user: {
      id: number;
      email: string;
      username: string | null;
      apiToken: string | null;
      isPublic: boolean;
    };
  };
}>();

// GET /api/tables/:tableKey - Get chart entries + user lamps
apiRoutes.get("/tables/:tableKey", async (c) => {
  const tableKey = c.req.param("tableKey");
  const username = c.req.query("user");

  const db = drizzle(c.env.DB);

  // Get charts for this table
  const chartRows = await db
    .select()
    .from(charts)
    .where(eq(charts.tableKey, tableKey));

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

      for (const l of userLamps) {
        lampMap.set(`${l.infinitasTitle}:${l.difficulty}`, {
          lamp: l.lamp,
          exScore: l.exScore,
          missCount: l.missCount,
        });
      }
    }
  }

  // Group by tier
  const tiers = new Map<string, Array<{
    id: number;
    title: string;
    infinitasTitle: string | null;
    difficulty: string;
    attributes: string | null;
    lamp: string;
    exScore: number | null;
    missCount: number | null;
  }>>();

  for (const chart of chartRows) {
    const key = `${chart.infinitasTitle ?? chart.title}:${chart.difficulty}`;
    const lampData = lampMap.get(key);

    const entry = {
      id: chart.id,
      title: chart.title,
      infinitasTitle: chart.infinitasTitle,
      difficulty: chart.difficulty,
      attributes: chart.attributes,
      lamp: lampData?.lamp ?? "NO PLAY",
      exScore: lampData?.exScore ?? null,
      missCount: lampData?.missCount ?? null,
    };

    const tierEntries = tiers.get(chart.tier);
    if (tierEntries) {
      tierEntries.push(entry);
    } else {
      tiers.set(chart.tier, [entry]);
    }
  }

  // Convert to array preserving tier order
  const result = Array.from(tiers.entries()).map(([tier, entries]) => ({
    tier,
    entries,
  }));

  return c.json({ tableKey, tiers: result });
});

// POST /api/lamps - Update single lamp (Bearer auth)
apiRoutes.post("/lamps", bearerAuth, async (c) => {
  const body = await c.req.json<LampInput>();
  if (!body.infinitasTitle || !body.difficulty || !body.lamp) {
    return c.json({ error: "infinitasTitle, difficulty, and lamp are required" }, 400);
  }

  if (!isValidLamp(body.lamp)) {
    return c.json({ error: "Invalid lamp value" }, 400);
  }

  const user = c.get("user");
  const db = drizzle(c.env.DB);

  // Check existing lamp
  const existing = await db
    .select()
    .from(lamps)
    .where(
      and(
        eq(lamps.userId, user.id),
        eq(lamps.infinitasTitle, body.infinitasTitle),
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
    infinitasTitle: body.infinitasTitle,
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
  const body = await c.req.json<LampInput[]>();
  if (!Array.isArray(body)) {
    return c.json({ error: "Expected an array of lamp entries" }, 400);
  }

  const user = c.get("user");
  const db = drizzle(c.env.DB);
  const now = new Date().toISOString();
  let updatedCount = 0;

  for (const entry of body) {
    if (!entry.infinitasTitle || !entry.difficulty || !entry.lamp) {
      continue;
    }
    if (!isValidLamp(entry.lamp)) {
      continue;
    }

    const existing = await db
      .select()
      .from(lamps)
      .where(
        and(
          eq(lamps.userId, user.id),
          eq(lamps.infinitasTitle, entry.infinitasTitle),
          eq(lamps.difficulty, entry.difficulty),
        ),
      )
      .limit(1);

    const existingLamp = existing[0];

    if (existingLamp) {
      if (isHigherLamp(entry.lamp, existingLamp.lamp)) {
        const updates: Record<string, unknown> = {
          lamp: entry.lamp,
          updatedAt: now,
        };
        if (entry.exScore !== undefined) {
          updates.exScore = entry.exScore;
        }
        if (entry.missCount !== undefined) {
          updates.missCount = entry.missCount;
        }
        await db.update(lamps).set(updates).where(eq(lamps.id, existingLamp.id));
        updatedCount++;
      }
    } else {
      await db.insert(lamps).values({
        userId: user.id,
        infinitasTitle: entry.infinitasTitle,
        difficulty: entry.difficulty,
        lamp: entry.lamp,
        exScore: entry.exScore ?? null,
        missCount: entry.missCount ?? null,
        updatedAt: now,
      });
      updatedCount++;
    }
  }

  return c.json({ updated: updatedCount, total: body.length });
});

// GET /api/lamps/updated-since - Polling endpoint
apiRoutes.get("/lamps/updated-since", async (c) => {
  const since = c.req.query("since");
  const username = c.req.query("user");

  if (!since || !username) {
    return c.json({ error: "since and user query params are required" }, 400);
  }

  const db = drizzle(c.env.DB);
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
      infinitasTitle: l.infinitasTitle,
      difficulty: l.difficulty,
      lamp: l.lamp,
      exScore: l.exScore,
      missCount: l.missCount,
      updatedAt: l.updatedAt,
    })),
  });
});

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
        title: string;
        infinitasTitle?: string;
        difficulty: string;
        tier: string;
        attributes?: string;
      }>
    >
  >();

  const db = drizzle(c.env.DB);
  let upsertCount = 0;

  for (const [tableKey, entries] of Object.entries(body)) {
    for (const entry of entries) {
      // Try to update existing, insert if not found
      const existing = await db
        .select()
        .from(charts)
        .where(and(eq(charts.tableKey, tableKey), eq(charts.title, entry.title)))
        .limit(1);

      if (existing[0]) {
        await db
          .update(charts)
          .set({
            infinitasTitle: entry.infinitasTitle ?? null,
            difficulty: entry.difficulty,
            tier: entry.tier,
            attributes: entry.attributes ?? null,
          })
          .where(eq(charts.id, existing[0].id));
      } else {
        await db.insert(charts).values({
          tableKey,
          title: entry.title,
          infinitasTitle: entry.infinitasTitle ?? null,
          difficulty: entry.difficulty,
          tier: entry.tier,
          attributes: entry.attributes ?? null,
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
  const db = drizzle(c.env.DB);

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
  return c.json({ apiToken: user.apiToken });
});

// POST /api/users/me/token/regenerate - Regenerate API token (session auth)
apiRoutes.post("/users/me/token/regenerate", sessionAuth, async (c) => {
  const user = c.get("user");
  const newToken = generateToken();
  const db = drizzle(c.env.DB);

  await db
    .update(users)
    .set({ apiToken: newToken })
    .where(eq(users.id, user.id));

  return c.json({ apiToken: newToken });
});
