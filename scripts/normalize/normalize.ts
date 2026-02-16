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
  sortOrder: number;
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

const TRACKER_JSON_PATH = path.resolve(__dirname, "tracker.json");
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

// --- Tracker loading ---

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

async function loadTracker(): Promise<Map<string, TrackerEntry>> {
  const content = await fs.readFile(TRACKER_JSON_PATH, "utf-8");
  const raw = JSON.parse(content) as {
    title: string;
    ratings: Record<Difficulty, number>;
    lamps: Record<Difficulty, string>;
  }[];

  const entries = new Map<string, TrackerEntry>();
  for (const item of raw) {
    entries.set(item.title, {
      title: item.title,
      ratings: item.ratings,
      lamps: {
        SPN: parseLamp(item.lamps.SPN),
        SPH: parseLamp(item.lamps.SPH),
        SPA: parseLamp(item.lamps.SPA),
        SPL: parseLamp(item.lamps.SPL),
      },
    });
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
  expectedRating: number,
  tracker: Map<string, TrackerEntry>,
  maxCandidates: number,
): LevenshteinCandidate[] {
  const normalizedInput = normalizeText(cleanTitle);
  const candidates: LevenshteinCandidate[] = [];

  for (const entry of tracker.values()) {
    // Only include candidates whose rating matches the expected value
    if (entry.ratings[difficulty] !== expectedRating) {
      continue;
    }
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
    expectedRating,
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
  console.log("Loading tracker.json...");
  const tracker = await loadTracker();
  console.log(`Loaded ${tracker.size} songs from tracker.json`);

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

  // Phase 1: Fetch all endpoints and attempt automatic matching
  interface PendingEntry {
    key: EndpointKey;
    apiEntry: IidxApiEntry;
    apiIndex: number;
    cleanTitle: string;
    difficulty: Difficulty;
    expectedRating: number;
  }

  const pending: PendingEntry[] = [];

  // Counters for summary
  let autoMatched = 0;
  let notInInfinitas = 0;
  let chartNotAvailable = 0;
  let ratingMismatch = 0;

  for (const [key, url] of Object.entries(ENDPOINTS) as [
    EndpointKey,
    string,
  ][]) {
    const expectedRating = EXPECTED_RATING[key];
    console.log(`\n--- ${key} (fetching ${url}) ---`);
    const entries = await fetchEndpoint(url);
    console.log(`Fetched ${entries.length} entries`);

    for (let apiIndex = 0; apiIndex < entries.length; apiIndex++) {
      const apiEntry = entries[apiIndex]!;
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

      if (match !== undefined) {
        if (match.rating === expectedRating) {
          // Auto-match success
          console.log(
            `\x1b[32m\u2713\x1b[0m ${apiEntry.title} \u2192 ${match.trackerTitle} (${difficulty} \u2606${match.rating})`,
          );
          result[key].push({
            title: apiEntry.title,
            infinitasTitle: match.trackerTitle,
            difficulty,
            tier: apiEntry.tier,
            attributes: apiEntry.attributes,
            sortOrder: apiIndex,
          });
          autoMatched++;
        } else if (match.rating === 0) {
          // Chart not available in INFINITAS
          console.log(
            `\x1b[33m-\x1b[0m Chart not available: ${apiEntry.title} (${difficulty} not in tracker)`,
          );
          chartNotAvailable++;
        } else {
          // Rating mismatch
          console.log(
            `\x1b[33m!\x1b[0m Rating mismatch: ${apiEntry.title} -> ${match.trackerTitle} ` +
              `(expected ${difficulty} \u2606${expectedRating}, got \u2606${match.rating})`,
          );
          ratingMismatch++;
        }
      } else {
        // No match found — check if a close candidate exists with correct rating
        const candidates = findLevenshteinCandidates(
          cleanTitle,
          difficulty,
          expectedRating,
          tracker,
          1,
        );
        const bestCandidate = candidates[0];

        const maxLen = Math.max(
          normalizeText(cleanTitle).length,
          bestCandidate !== undefined
            ? normalizeText(bestCandidate.title).length
            : 0,
        );
        const ratio = maxLen > 0 && bestCandidate !== undefined
          ? bestCandidate.distance / maxLen
          : 1;

        if (bestCandidate !== undefined && ratio <= 0.30) {
          // Close match exists — needs interactive resolution
          pending.push({
            key,
            apiEntry,
            apiIndex,
            cleanTitle,
            difficulty,
            expectedRating,
          });
        } else {
          // Not in INFINITAS
          console.log(
            `\x1b[90m-\x1b[0m Not in INFINITAS: ${apiEntry.title}`,
          );
          notInInfinitas++;
        }
      }
    }
  }

  // Phase 2: Interactive resolution for unmatched entries
  if (pending.length === 0) {
    console.log("\nAll entries matched automatically!");
  } else {
    console.log(
      `\n\x1b[33m${pending.length} entries\x1b[0m need interactive resolution:`,
    );
    for (const p of pending) {
      console.log(`  - ${p.apiEntry.title} (${p.difficulty} \u2606${p.expectedRating}) [${p.key}]`);
    }
  }

  try {
    for (const p of pending) {
      const resolved = await resolveInteractively(
        p.apiEntry.title,
        p.difficulty,
        p.expectedRating,
        tracker,
        rl,
      );

      if (resolved !== undefined) {
        console.log(
          `\x1b[32m\u2713\x1b[0m Resolved: ${p.apiEntry.title} \u2192 ${resolved}`,
        );
        result[p.key].push({
          title: p.apiEntry.title,
          infinitasTitle: resolved,
          difficulty: p.difficulty,
          tier: p.apiEntry.tier,
          attributes: p.apiEntry.attributes,
          sortOrder: p.apiIndex,
        });
      } else {
        console.log(`\x1b[33m-\x1b[0m Skipped: ${p.apiEntry.title}`);
      }
    }
  } finally {
    rl.close();
  }

  // Write output
  await fs.writeFile(OUTPUT_PATH, JSON.stringify(result, null, 2) + "\n");
  console.log(`\nWrote ${OUTPUT_PATH}`);

  // Summary
  console.log("\nSummary:");
  console.log(`  Auto-matched: ${autoMatched}`);
  console.log(`  Not in INFINITAS: ${notInInfinitas}`);
  console.log(`  Chart not available: ${chartNotAvailable}`);
  console.log(`  Rating mismatch: ${ratingMismatch}`);
  console.log(`  Need interactive resolution: ${pending.length}`);
  console.log("\nPer endpoint:");
  for (const [key, entries] of Object.entries(result) as [
    EndpointKey,
    MappedEntry[],
  ][]) {
    console.log(`  ${key}: ${entries.length} entries`);
  }
}

normalize();
