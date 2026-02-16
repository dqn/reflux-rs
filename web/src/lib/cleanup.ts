import { CLEANUP_PROBABILITY } from "./constants";

// Probabilistic cleanup of expired device codes
export async function maybeCleanupDeviceCodes(db: D1Database): Promise<void> {
  if (Math.random() >= CLEANUP_PROBABILITY) {
    return;
  }
  await cleanupExpiredDeviceCodes(db);
}

// Delete all expired device codes
export async function cleanupExpiredDeviceCodes(
  db: D1Database,
): Promise<void> {
  const now = new Date().toISOString();
  await db
    .prepare("DELETE FROM device_codes WHERE expires_at < ?")
    .bind(now)
    .run()
    .catch(() => {});
}

// Delete old rate limit entries
export async function cleanupRateLimits(db: D1Database): Promise<void> {
  const cutoff = new Date(Date.now() - 3600 * 1000).toISOString(); // 1 hour ago
  await db
    .prepare("DELETE FROM rate_limits WHERE created_at < ?")
    .bind(cutoff)
    .run()
    .catch(() => {});
}
