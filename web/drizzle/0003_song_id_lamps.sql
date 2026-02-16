DROP TABLE `lamps`;
--> statement-breakpoint
DROP TABLE `charts`;
--> statement-breakpoint
CREATE TABLE `charts` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`table_key` text NOT NULL,
	`song_id` integer NOT NULL,
	`title` text NOT NULL,
	`difficulty` text NOT NULL,
	`tier` text NOT NULL,
	`attributes` text
);
--> statement-breakpoint
CREATE UNIQUE INDEX `charts_table_key_song_diff_idx` ON `charts` (`table_key`,`song_id`,`difficulty`);
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
CREATE UNIQUE INDEX `lamps_user_song_diff_idx` ON `lamps` (`user_id`,`song_id`,`difficulty`);
--> statement-breakpoint
CREATE INDEX `lamps_user_updated_at_idx` ON `lamps` (`user_id`,`updated_at`);
