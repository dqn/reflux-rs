import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const target = process.argv[2];

if (target !== "local" && target !== "remote") {
  console.error("Usage: node ./scripts/sync-charts-to-d1.mjs <local|remote>");
  process.exit(1);
}

const webDir = path.resolve(__dirname, "..");
const repoDir = path.resolve(webDir, "..");
const mappingPath = path.resolve(repoDir, "scripts", "normalize", "title-mapping.json");

if (!fs.existsSync(mappingPath)) {
  console.error(`Mapping file not found: ${mappingPath}`);
  process.exit(1);
}

const escapeSql = (value) => String(value).replaceAll("'", "''");

const mapping = JSON.parse(fs.readFileSync(mappingPath, "utf8"));
const rows = [];

for (const [tableKey, entries] of Object.entries(mapping)) {
  for (const entry of entries) {
    const infinitasTitle = entry.infinitasTitle
      ? `'${escapeSql(entry.infinitasTitle)}'`
      : "NULL";
    const attributes = entry.attributes
      ? `'${escapeSql(entry.attributes)}'`
      : "NULL";

    rows.push(
      `('${escapeSql(tableKey)}','${escapeSql(entry.title)}',${infinitasTitle},'${escapeSql(entry.difficulty)}','${escapeSql(entry.tier)}',${attributes})`,
    );
  }
}

if (rows.length === 0) {
  console.error("No rows found in mapping file.");
  process.exit(1);
}

const chunkSize = 100;
const statements = [];

for (let i = 0; i < rows.length; i += chunkSize) {
  const chunk = rows.slice(i, i + chunkSize);
  statements.push([
    "INSERT INTO charts (table_key, title, infinitas_title, difficulty, tier, attributes) VALUES",
    chunk.join(",\n"),
    "ON CONFLICT(table_key, title) DO UPDATE SET",
    "  infinitas_title=excluded.infinitas_title,",
    "  difficulty=excluded.difficulty,",
    "  tier=excluded.tier,",
    "  attributes=excluded.attributes;",
  ].join("\n"));
}

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "infst-charts-sync-"));
const sqlPath = path.join(tempDir, "charts-upsert.sql");
fs.writeFileSync(sqlPath, `${statements.join("\n\n")}\n`);

console.log(`Sync target: ${target}`);
console.log(`Mapping rows: ${rows.length}`);
console.log(`Statements: ${statements.length}`);

const result = spawnSync(
  "npx",
  [
    "wrangler",
    "d1",
    "execute",
    "infst-db",
    `--${target}`,
    "--file",
    sqlPath,
  ],
  {
    cwd: webDir,
    stdio: "inherit",
  },
);

fs.rmSync(tempDir, { recursive: true, force: true });

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

console.log("charts sync completed.");
