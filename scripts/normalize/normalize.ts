import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as readline from "node:readline/promises";
import { fileURLToPath } from "node:url";
import { distance } from "fastest-levenshtein";

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

interface IidxApiEntry {
  title: string;
  tier: string;
  attributes: string[];
}

interface MappedEntry {
  title: string;
  infinitasTitle: string;
  difficulty: Difficulty;
  tier: string;
  attributes: string[];
}

interface TitleMapping {
  "sp12-hard": MappedEntry[];
  "sp12-normal": MappedEntry[];
  "sp11-hard": MappedEntry[];
  "sp11-normal": MappedEntry[];
}

type EndpointKey = keyof TitleMapping;

// --- Constants ---

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TRACKER_TSV_PATH = path.resolve(__dirname, "../../.agent/tracker.tsv");
const OUTPUT_PATH = path.resolve(__dirname, "title-mapping.json");

const ENDPOINTS: Record<EndpointKey, string> = {
  "sp11-normal": "https://dqn.github.io/iidxapi/sp11/normal.json",
  "sp11-hard": "https://dqn.github.io/iidxapi/sp11/hard.json",
  "sp12-normal": "https://dqn.github.io/iidxapi/sp12/normal.json",
  "sp12-hard": "https://dqn.github.io/iidxapi/sp12/hard.json",
};

const EXPECTED_RATING: Record<EndpointKey, number> = {
  "sp11-normal": 11,
  "sp11-hard": 11,
  "sp12-normal": 12,
  "sp12-hard": 12,
};

// Fullwidth to halfwidth character map
const FULLWIDTH_TO_HALFWIDTH: ReadonlyMap<string, string> = new Map([
  ["\uff5e", "~"], // ～ → ~
  ["\uff08", "("], // （ → (
  ["\uff09", ")"], // ） → )
  ["\uff01", "!"], // ！ → !
  ["\u3000", " "], // fullwidth space → halfwidth space
]);

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

// --- Normalization ---

function normalizeText(text: string): string {
  let result = "";
  for (const ch of text) {
    const mapped = FULLWIDTH_TO_HALFWIDTH.get(ch);
    if (mapped !== undefined) {
      result += mapped;
    } else {
      result += ch;
    }
  }
  return result.trim().toLowerCase();
}

// --- Suffix analysis ---

interface SuffixAnalysis {
  cleanTitle: string;
  difficulty: Difficulty;
}

function analyzeSuffix(title: string): SuffixAnalysis {
  if (title.endsWith("(L)")) {
    return {
      cleanTitle: title.slice(0, -3).trim(),
      difficulty: "SPL",
    };
  }
  if (title.endsWith("(H)")) {
    return {
      cleanTitle: title.slice(0, -3).trim(),
      difficulty: "SPH",
    };
  }
  return { cleanTitle: title, difficulty: "SPA" };
}

// --- Tracker TSV parsing ---

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

async function parseTrackerTsv(): Promise<Map<string, TrackerEntry>> {
  const content = await fs.readFile(TRACKER_TSV_PATH, "utf-8");
  const lines = content.split("\n");
  const entries = new Map<string, TrackerEntry>();

  // Skip header line
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i];
    if (line === undefined || line.trim() === "") {
      continue;
    }

    const cols = line.split("\t");
    const title = cols[COL.TITLE];
    if (title === undefined || title.trim() === "") {
      continue;
    }

    const entry: TrackerEntry = {
      title: title.trim(),
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
    };

    entries.set(entry.title, entry);
  }

  return entries;
}

// --- Matching ---

interface MatchResult {
  trackerTitle: string;
  rating: number;
}

function findExactMatch(
  cleanTitle: string,
  difficulty: Difficulty,
  tracker: Map<string, TrackerEntry>,
): MatchResult | undefined {
  const entry = tracker.get(cleanTitle);
  if (entry === undefined) {
    return undefined;
  }
  return { trackerTitle: entry.title, rating: entry.ratings[difficulty] };
}

function findNormalizedMatch(
  cleanTitle: string,
  difficulty: Difficulty,
  normalizedIndex: Map<string, TrackerEntry>,
): MatchResult | undefined {
  const normalizedKey = normalizeText(cleanTitle);
  const entry = normalizedIndex.get(normalizedKey);
  if (entry === undefined) {
    return undefined;
  }
  return { trackerTitle: entry.title, rating: entry.ratings[difficulty] };
}

interface LevenshteinCandidate {
  title: string;
  rating: number;
  distance: number;
}

function findLevenshteinCandidates(
  cleanTitle: string,
  difficulty: Difficulty,
  tracker: Map<string, TrackerEntry>,
  maxCandidates: number,
): LevenshteinCandidate[] {
  const normalizedInput = normalizeText(cleanTitle);
  const candidates: LevenshteinCandidate[] = [];

  for (const entry of tracker.values()) {
    const normalizedEntry = normalizeText(entry.title);
    const dist = distance(normalizedInput, normalizedEntry);
    candidates.push({
      title: entry.title,
      rating: entry.ratings[difficulty],
      distance: dist,
    });
  }

  candidates.sort((a, b) => a.distance - b.distance);
  return candidates.slice(0, maxCandidates);
}

// --- Interactive resolution ---

async function resolveInteractively(
  apiTitle: string,
  difficulty: Difficulty,
  expectedRating: number,
  tracker: Map<string, TrackerEntry>,
  rl: readline.Interface,
): Promise<string | undefined> {
  const { cleanTitle } = analyzeSuffix(apiTitle);
  const candidates = findLevenshteinCandidates(
    cleanTitle,
    difficulty,
    tracker,
    10,
  );

  console.log(
    `\n\x1b[31m\u2717\x1b[0m No match: ${apiTitle} (${difficulty} \u2606${expectedRating})`,
  );
  console.log("  Candidates:");

  for (let i = 0; i < candidates.length; i++) {
    const c = candidates[i]!;
    console.log(
      `    ${i + 1}. ${c.title} (${difficulty} Rating: ${c.rating}, dist: ${c.distance})`,
    );
  }
  console.log(`    ${candidates.length + 1}. Skip`);

  const answer = await rl.question("  Enter choice: ");
  const choice = Math.trunc(Number(answer));

  if (choice >= 1 && choice <= candidates.length) {
    const selected = candidates[choice - 1]!;
    return selected.title;
  }

  return undefined;
}

// --- Fetch endpoints ---

async function fetchEndpoint(url: string): Promise<IidxApiEntry[]> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Failed to fetch ${url}: ${response.status}`);
  }
  return (await response.json()) as IidxApiEntry[];
}

// --- Main ---

export async function normalize(): Promise<void> {
  console.log("Loading tracker.tsv...");
  const tracker = await parseTrackerTsv();
  console.log(`Loaded ${tracker.size} songs from tracker.tsv`);

  // Build normalized index for fast lookup
  const normalizedIndex = new Map<string, TrackerEntry>();
  for (const entry of tracker.values()) {
    normalizedIndex.set(normalizeText(entry.title), entry);
  }

  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });

  const result: TitleMapping = {
    "sp12-hard": [],
    "sp12-normal": [],
    "sp11-hard": [],
    "sp11-normal": [],
  };

  try {
    for (const [key, url] of Object.entries(ENDPOINTS) as [
      EndpointKey,
      string,
    ][]) {
      const expectedRating = EXPECTED_RATING[key];
      console.log(`\n--- ${key} (fetching ${url}) ---`);
      const entries = await fetchEndpoint(url);
      console.log(`Fetched ${entries.length} entries`);

      for (const apiEntry of entries) {
        const { cleanTitle, difficulty } = analyzeSuffix(apiEntry.title);

        // 1. Try exact match
        let match = findExactMatch(cleanTitle, difficulty, tracker);

        // 2. Try normalized match
        if (match === undefined) {
          match = findNormalizedMatch(
            cleanTitle,
            difficulty,
            normalizedIndex,
          );
        }

        // Validate rating for automatic matches
        if (match !== undefined && match.rating !== expectedRating) {
          console.log(
            `\x1b[33m!\x1b[0m Rating mismatch: ${apiEntry.title} -> ${match.trackerTitle} ` +
              `(expected ${difficulty} \u2606${expectedRating}, got \u2606${match.rating})`,
          );
          match = undefined;
        }

        if (match !== undefined) {
          console.log(
            `\x1b[32m\u2713\x1b[0m ${apiEntry.title} \u2192 ${match.trackerTitle} (${difficulty} \u2606${match.rating})`,
          );
          result[key].push({
            title: apiEntry.title,
            infinitasTitle: match.trackerTitle,
            difficulty,
            tier: apiEntry.tier,
            attributes: apiEntry.attributes,
          });
          continue;
        }

        // 3. Interactive resolution
        const resolved = await resolveInteractively(
          apiEntry.title,
          difficulty,
          expectedRating,
          tracker,
          rl,
        );

        if (resolved !== undefined) {
          console.log(
            `\x1b[32m\u2713\x1b[0m Resolved: ${apiEntry.title} \u2192 ${resolved}`,
          );
          result[key].push({
            title: apiEntry.title,
            infinitasTitle: resolved,
            difficulty,
            tier: apiEntry.tier,
            attributes: apiEntry.attributes,
          });
        } else {
          console.log(`\x1b[33m-\x1b[0m Skipped: ${apiEntry.title}`);
        }
      }
    }
  } finally {
    rl.close();
  }

  // Write output
  await fs.writeFile(OUTPUT_PATH, JSON.stringify(result, null, 2) + "\n");
  console.log(`\nWrote ${OUTPUT_PATH}`);

  // Summary
  for (const [key, entries] of Object.entries(result) as [
    EndpointKey,
    MappedEntry[],
  ][]) {
    console.log(`  ${key}: ${entries.length} entries`);
  }
}

normalize();
