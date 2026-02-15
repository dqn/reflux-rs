import {
  sqliteTable,
  text,
  integer,
  uniqueIndex,
} from "drizzle-orm/sqlite-core";
import { sql } from "drizzle-orm";

export const users = sqliteTable("users", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  email: text("email").notNull().unique(),
  username: text("username").unique(),
  apiToken: text("api_token").unique(),
  isPublic: integer("is_public", { mode: "boolean" }).notNull().default(true),
  createdAt: text("created_at")
    .notNull()
    .default(sql`(datetime('now'))`),
});

export const magicLinks = sqliteTable("magic_links", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  email: text("email").notNull(),
  token: text("token").notNull().unique(),
  expiresAt: text("expires_at").notNull(),
  usedAt: text("used_at"),
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
    title: text("title").notNull(),
    infinitasTitle: text("infinitas_title"),
    difficulty: text("difficulty").notNull(),
    tier: text("tier").notNull(),
    attributes: text("attributes"),
  },
  (table) => [
    uniqueIndex("charts_table_key_title_idx").on(table.tableKey, table.title),
  ],
);

export const lamps = sqliteTable(
  "lamps",
  {
    id: integer("id").primaryKey({ autoIncrement: true }),
    userId: integer("user_id").notNull(),
    infinitasTitle: text("infinitas_title").notNull(),
    difficulty: text("difficulty").notNull(),
    lamp: text("lamp").notNull(),
    exScore: integer("ex_score"),
    missCount: integer("miss_count"),
    updatedAt: text("updated_at")
      .notNull()
      .default(sql`(datetime('now'))`),
  },
  (table) => [
    uniqueIndex("lamps_user_title_diff_idx").on(
      table.userId,
      table.infinitasTitle,
      table.difficulty,
    ),
  ],
);
