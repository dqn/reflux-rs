import {
  sqliteTable,
  text,
  integer,
  uniqueIndex,
  index,
} from "drizzle-orm/sqlite-core";
import { sql } from "drizzle-orm";

export const users = sqliteTable("users", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  email: text("email").notNull().unique(),
  username: text("username").notNull().unique(),
  passwordHash: text("password_hash").notNull(),
  apiToken: text("api_token").unique(),
  apiTokenCreatedAt: text("api_token_created_at"),
  isPublic: integer("is_public", { mode: "boolean" }).notNull().default(true),
  createdAt: text("created_at")
    .notNull()
    .default(sql`(datetime('now'))`),
});

export const deviceCodes = sqliteTable("device_codes", {
  deviceCode: text("device_code").primaryKey(),
  userCode: text("user_code").notNull(),
  userId: integer("user_id"),
  apiToken: text("api_token"),
  expiresAt: text("expires_at").notNull(),
  createdAt: text("created_at")
    .notNull()
    .default(sql`(datetime('now'))`),
});

export const charts = sqliteTable(
  "charts",
  {
    id: integer("id").primaryKey({ autoIncrement: true }),
    tableKey: text("table_key").notNull(),
    songId: integer("song_id").notNull(),
    title: text("title").notNull(),
    difficulty: text("difficulty").notNull(),
    tier: text("tier").notNull(),
    attributes: text("attributes"),
    sortOrder: integer("sort_order"),
  },
  (table) => [
    uniqueIndex("charts_table_key_song_diff_idx").on(
      table.tableKey,
      table.songId,
      table.difficulty,
    ),
  ],
);

export const lamps = sqliteTable(
  "lamps",
  {
    id: integer("id").primaryKey({ autoIncrement: true }),
    userId: integer("user_id").notNull(),
    songId: integer("song_id").notNull(),
    difficulty: text("difficulty").notNull(),
    lamp: text("lamp").notNull(),
    exScore: integer("ex_score"),
    missCount: integer("miss_count"),
    updatedAt: text("updated_at")
      .notNull()
      .default(sql`(datetime('now'))`),
  },
  (table) => [
    uniqueIndex("lamps_user_song_diff_idx").on(
      table.userId,
      table.songId,
      table.difficulty,
    ),
    index("lamps_user_updated_at_idx").on(table.userId, table.updatedAt),
  ],
);

export const rateLimits = sqliteTable("rate_limits", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  key: text("key").notNull(),
  createdAt: text("created_at").notNull(),
});
