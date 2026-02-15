CREATE TABLE `charts` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`table_key` text NOT NULL,
	`title` text NOT NULL,
	`infinitas_title` text,
	`difficulty` text NOT NULL,
	`tier` text NOT NULL,
	`attributes` text
);
--> statement-breakpoint
CREATE UNIQUE INDEX `charts_table_key_title_idx` ON `charts` (`table_key`,`title`);--> statement-breakpoint
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
	`infinitas_title` text NOT NULL,
	`difficulty` text NOT NULL,
	`lamp` text NOT NULL,
	`ex_score` integer,
	`miss_count` integer,
	`updated_at` text DEFAULT (datetime('now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `lamps_user_title_diff_idx` ON `lamps` (`user_id`,`infinitas_title`,`difficulty`);--> statement-breakpoint
CREATE TABLE `users` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`email` text NOT NULL,
	`username` text NOT NULL,
	`password_hash` text NOT NULL,
	`api_token` text,
	`is_public` integer DEFAULT true NOT NULL,
	`created_at` text DEFAULT (datetime('now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `users_email_unique` ON `users` (`email`);--> statement-breakpoint
CREATE UNIQUE INDEX `users_username_unique` ON `users` (`username`);--> statement-breakpoint
CREATE UNIQUE INDEX `users_api_token_unique` ON `users` (`api_token`);