import { Hono } from "hono";
import { eq, asc } from "drizzle-orm";

import type { AppEnv, SessionUser } from "../lib/types";
import { optionalSession, sessionAuth } from "../middleware/session";
import { users, charts, lamps } from "../db/schema";
import { buildLampMap, groupChartsByTier, formatTableKey, sortTableKeys } from "../lib/chart-table";
import { Layout } from "../components/Layout";
import { LoginPage } from "../components/LoginPage";
import { RegisterPage } from "../components/RegisterPage";
import { SettingsPage } from "../components/SettingsPage";
import { GuidePage } from "../components/GuidePage";
import { TableView } from "../components/TableView";

export const pageRoutes = new Hono<AppEnv>();

// GET / - Top page
pageRoutes.get("/", optionalSession, (c) => {
  const user = c.get("user") as SessionUser | null;

  return c.html(
    <Layout user={user}>
      <div style="margin-top:24px;">
        <h2 style="margin-bottom:16px;font-weight:300;">infst - IIDX INFINITAS Score Tracker</h2>
        <p style="color:#999;margin-bottom:24px;">
          Track your clear lamps on difficulty tables.
        </p>

        {/* User search */}
        <div class="card">
          <h3 style="font-size:1rem;margin-bottom:8px;">Find a player</h3>
          <form id="search-form" style="display:flex;gap:8px;">
            <input
              type="text"
              name="username"
              placeholder="Username"
              style="flex:1;"
            />
            <button type="submit">View</button>
          </form>
          <script src="/search-form.js"></script>
        </div>
      </div>
    </Layout>,
  );
});

// GET /login - Login page
pageRoutes.get("/login", (c) => {
  return c.html(<LoginPage />);
});

// GET /register - Registration page
pageRoutes.get("/register", (c) => {
  return c.html(<RegisterPage />);
});

// GET /settings - Settings page (session required)
pageRoutes.get("/settings", sessionAuth, async (c) => {
  const user = c.get("user") as SessionUser;
  return c.html(<SettingsPage user={user} />);
});

// GET /guide - Guide page
pageRoutes.get("/guide", optionalSession, (c) => {
  const user = c.get("user") as SessionUser | null;
  return c.html(<GuidePage user={user} />);
});

// GET /:username - User's table list
pageRoutes.get("/:username", optionalSession, async (c) => {
  const username = c.req.param("username");
  const sessionUser = c.get("user") as SessionUser | null;

  const db = c.get("db");
  const userResult = await db
    .select()
    .from(users)
    .where(eq(users.username, username))
    .limit(1);

  const targetUser = userResult[0];
  if (!targetUser) {
    return c.html(
      <Layout user={sessionUser}>
        <div style="margin-top:48px;text-align:center;">
          <h2>User not found</h2>
          <p style="color:#666;margin-top:8px;">
            The user "{username}" does not exist.
          </p>
        </div>
      </Layout>,
      404,
    );
  }

  if (!targetUser.isPublic && targetUser.id !== sessionUser?.id) {
    return c.html(
      <Layout user={sessionUser}>
        <div style="margin-top:48px;text-align:center;">
          <h2>Private Profile</h2>
          <p style="color:#666;margin-top:8px;">
            This user's profile is private.
          </p>
        </div>
      </Layout>,
      403,
    );
  }

  // Get distinct table keys for this user's lamps
  const userCharts = sortTableKeys(
    await db
      .select({ tableKey: charts.tableKey })
      .from(charts)
      .groupBy(charts.tableKey),
  );

  return c.html(
    <Layout user={sessionUser}>
      <div style="margin-top:24px;">
        <h2 style="margin-bottom:16px;">{username}</h2>
        <h3 style="font-size:1rem;margin-bottom:12px;color:#999;">Difficulty Tables</h3>
        {userCharts.length === 0 ? (
          <p style="color:#666;">No tables available.</p>
        ) : (
          <ul style="list-style:none;display:flex;flex-direction:column;gap:8px;">
            {userCharts.map((chart) => (
              <li>
                <a
                  href={`/${username}/${chart.tableKey}`}
                  style="display:block;padding:12px 16px;background:#1a1a1a;border:1px solid #2a2a2a;border-radius:8px;color:#e0e0e0;text-decoration:none;transition:border-color 0.15s;"
                  onmouseover="this.style.borderColor='#444'"
                  onmouseout="this.style.borderColor='#2a2a2a'"
                >
                  {formatTableKey(chart.tableKey)}
                </a>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Layout>,
  );
});

// GET /:username/:tableKey - Difficulty table view
pageRoutes.get("/:username/:tableKey", optionalSession, async (c) => {
  const username = c.req.param("username");
  const tableKey = c.req.param("tableKey");
  const sessionUser = c.get("user") as SessionUser | null;

  const db = c.get("db");

  // Find user
  const userResult = await db
    .select()
    .from(users)
    .where(eq(users.username, username))
    .limit(1);

  const targetUser = userResult[0];
  if (!targetUser) {
    return c.html(
      <Layout user={sessionUser}>
        <div style="margin-top:48px;text-align:center;">
          <h2>User not found</h2>
          <p style="color:#666;margin-top:8px;">
            The user "{username}" does not exist.
          </p>
        </div>
      </Layout>,
      404,
    );
  }

  if (!targetUser.isPublic && targetUser.id !== sessionUser?.id) {
    return c.html(
      <Layout user={sessionUser}>
        <div style="margin-top:48px;text-align:center;">
          <h2>Private Profile</h2>
          <p style="color:#666;margin-top:8px;">
            This user's profile is private.
          </p>
        </div>
      </Layout>,
      403,
    );
  }

  // Get charts
  const chartRows = await db
    .select()
    .from(charts)
    .where(eq(charts.tableKey, tableKey))
    .orderBy(asc(charts.sortOrder));

  if (chartRows.length === 0) {
    return c.html(
      <Layout user={sessionUser}>
        <div style="margin-top:48px;text-align:center;">
          <h2>Table not found</h2>
          <p style="color:#666;margin-top:8px;">
            The table "{tableKey}" does not exist.
          </p>
        </div>
      </Layout>,
      404,
    );
  }

  // Get user lamps
  const userLamps = await db
    .select()
    .from(lamps)
    .where(eq(lamps.userId, targetUser.id));

  const lampMap = buildLampMap(userLamps);
  const tiers = groupChartsByTier(chartRows, lampMap);

  return c.html(
    <Layout title={`${username} - ${formatTableKey(tableKey)}`} user={sessionUser}>
      <div style="margin-top:16px;">
        <p style="margin-bottom:8px;font-size:0.9rem;color:#999;">
          <a href={`/${username}`} style="color:#999;">{username}</a>{" "}
          <span style="color:#666;">/</span>{" "}
          {formatTableKey(tableKey)}
        </p>
        <TableView tableKey={tableKey} tiers={tiers} username={username} />
      </div>
    </Layout>,
  );
});
