CREATE TABLE `charts` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`table_key` text NOT NULL,
	`song_id` integer NOT NULL,
	`title` text NOT NULL,
	`difficulty` text NOT NULL,
	`tier` text NOT NULL,
	`attributes` text,
	`sort_order` integer
);
--> statement-breakpoint
CREATE UNIQUE INDEX `charts_table_key_song_diff_idx` ON `charts` (`table_key`,`song_id`,`difficulty`);--> statement-breakpoint
CREATE TABLE `device_codes` (
	`device_code` text PRIMARY KEY NOT NULL,
	`user_code` text NOT NULL,
	`user_id` integer,
	`api_token` text,
	`expires_at` text NOT NULL,
	`created_at` text DEFAULT (datetime('now')) NOT NULL
);
--> statement-breakpoint
CREATE TABLE `lamps` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`user_id` integer NOT NULL,
	`song_id` integer NOT NULL,
	`difficulty` text NOT NULL,
	`lamp` text NOT NULL,
	`ex_score` integer,
	`miss_count` integer,
	`updated_at` text DEFAULT (datetime('now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `lamps_user_song_diff_idx` ON `lamps` (`user_id`,`song_id`,`difficulty`);--> statement-breakpoint
CREATE INDEX `lamps_user_updated_at_idx` ON `lamps` (`user_id`,`updated_at`);--> statement-breakpoint
CREATE TABLE `rate_limits` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`key` text NOT NULL,
	`created_at` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE `users` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`email` text NOT NULL,
	`username` text NOT NULL,
	`password_hash` text NOT NULL,
	`api_token` text,
	`api_token_created_at` text,
	`is_public` integer DEFAULT true NOT NULL,
	`created_at` text DEFAULT (datetime('now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `users_email_unique` ON `users` (`email`);--> statement-breakpoint
CREATE UNIQUE INDEX `users_username_unique` ON `users` (`username`);--> statement-breakpoint
CREATE UNIQUE INDEX `users_api_token_unique` ON `users` (`api_token`);