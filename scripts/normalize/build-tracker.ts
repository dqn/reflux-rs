import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// --- Types ---

type Difficulty = "SPN" | "SPH" | "SPA" | "SPL";

type Lamp =
  | "NO PLAY"
  | "FAILED"
  | "ASSIST"
  | "EASY"
  | "CLEAR"
  | "HARD"
  | "EX HARD"
  | "FC"
  | "PFC";

interface TrackerEntry {
  title: string;
  ratings: Record<Difficulty, number>;
  lamps: Record<Difficulty, Lamp>;
}

// --- Constants ---

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TRACKER_TSV_PATH = path.resolve(__dirname, "../../.agent/tracker.tsv");
const OUTPUT_PATH = path.resolve(__dirname, "tracker.json");

// Column indices in tracker.tsv (0-based)
const COL = {
  TITLE: 0,
  SPN_RATING: 17,
  SPN_LAMP: 18,
  SPH_RATING: 25,
  SPH_LAMP: 26,
  SPA_RATING: 33,
  SPA_LAMP: 34,
  SPL_RATING: 41,
  SPL_LAMP: 42,
} as const;

// Mojibake fixes: full title mapping (tracker title -> correct Unicode title)
const MOJIBAKE_FIXES: ReadonlyMap<string, string> = new Map([
  // Latin characters (accents, special characters)
  ["?bertreffen", "Übertreffen"],
  ["?THER", "ÆTHER"],
  ["?u Legends", "Ōu Legends"],
  ["?Viva!", "¡Viva!"],
  ["?影", "焱影"],
  ["ACT?", "ACTØ"],
  ["Amor De Ver?o", "Amor De Verão"],
  ["Dans la nuit de l'?ternit?", "Dans la nuit de l'éternité"],
  ["Geirsk?gul", "Geirskögul"],
  ["Ignis†Ir?", "Ignis†Iræ"],
  ["M?ch? M?nky", "Mächö Mönky"],
  ["P?rvat?", "Pārvatī"],
  ["POL?AMAИIA", "POLꓘAMAИIA"],
  ["Pr?ludium", "Präludium"],
  ["Raison d'?tre～交差する宿命～", "Raison d'être～交差する宿命～"],
  ["V?ID", "VØID"],
  ["旋律のドグマ～Mis?rables～", "旋律のドグマ～Misérables～"],
  ["u?n", "uən"],
  // Symbols (hearts)
  ["LOVE?SHINE", "LOVE♡SHINE"],
  ["Sweet Sweet?Magic", "Sweet Sweet♡Magic"],
  ["Raspberry?Heart(English version)", "Raspberry♡Heart(English version)"],
  ["Double??Loving Heart", "Double♡♡Loving Heart"],
  ["Love?km", "Love♥km"],
  ["超!!遠距離らぶ?メ～ル", "超!!遠距離らぶ♡メ～ル"],
  ["キャトられ?恋はモ～モク", "キャトられ♥恋はモ～モク"],
  ["表裏一体！？怪盗いいんちょの悩み?", "表裏一体！？怪盗いいんちょの悩み♥"],
  // Compound (multiple types of mojibake)
  ["?LOVE? シュガ→?", "♥LOVE² シュガ→♥"],
  ["ジオメトリック?ティーパーティー", "ジオメトリック∮ティーパーティー"],
]);

// --- Lamp parsing ---

function parseLamp(value: string | undefined): Lamp {
  const trimmed = (value ?? "").trim();
  const validLamps: Lamp[] = [
    "NO PLAY",
    "FAILED",
    "ASSIST",
    "EASY",
    "CLEAR",
    "HARD",
    "EX HARD",
    "FC",
    "PFC",
  ];
  if (validLamps.includes(trimmed as Lamp)) {
    return trimmed as Lamp;
  }
  return "NO PLAY";
}

// --- Main ---

async function buildTracker(): Promise<void> {
  console.log("Reading tracker.tsv...");
  const content = await fs.readFile(TRACKER_TSV_PATH, "utf-8");
  const lines = content.split("\n");

  const entries: TrackerEntry[] = [];
  let mojibakeFixed = 0;

  // Skip header line
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i];
    if (line === undefined || line.trim() === "") {
      continue;
    }

    const cols = line.split("\t");
    let title = cols[COL.TITLE];
    if (title === undefined || title.trim() === "") {
      continue;
    }
    title = title.trim();

    // Apply mojibake fix
    const fixed = MOJIBAKE_FIXES.get(title);
    if (fixed !== undefined) {
      console.log(`  Fixed: ${title} → ${fixed}`);
      title = fixed;
      mojibakeFixed++;
    }

    entries.push({
      title,
      ratings: {
        SPN: Math.trunc(Number(cols[COL.SPN_RATING] ?? "0")),
        SPH: Math.trunc(Number(cols[COL.SPH_RATING] ?? "0")),
        SPA: Math.trunc(Number(cols[COL.SPA_RATING] ?? "0")),
        SPL: Math.trunc(Number(cols[COL.SPL_RATING] ?? "0")),
      },
      lamps: {
        SPN: parseLamp(cols[COL.SPN_LAMP]),
        SPH: parseLamp(cols[COL.SPH_LAMP]),
        SPA: parseLamp(cols[COL.SPA_LAMP]),
        SPL: parseLamp(cols[COL.SPL_LAMP]),
      },
    });
  }

  await fs.writeFile(OUTPUT_PATH, JSON.stringify(entries, null, 2) + "\n");

  console.log(`\nWrote ${entries.length} entries to ${OUTPUT_PATH}`);
  console.log(`Mojibake fixed: ${mojibakeFixed}`);

  // Verify no remaining mojibake (excluding legitimate ?)
  const legitimateQuestionMarks = new Set([
    "Wanna Party?",
    "BLACK or WHITE?",
    "My Sweet Bird?",
    "がっつり陰キャ!? 怪盗いいんちょの億劫^^;",
  ]);
  const remaining = entries.filter(
    (e) => e.title.includes("?") && !legitimateQuestionMarks.has(e.title),
  );
  if (remaining.length > 0) {
    console.warn(`\nWARNING: ${remaining.length} titles still contain '?':`);
    for (const e of remaining) {
      console.warn(`  - ${e.title}`);
    }
  } else {
    console.log("All mojibake titles fixed successfully.");
  }
}

buildTracker();
