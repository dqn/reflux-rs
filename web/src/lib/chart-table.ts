interface ChartRow {
  id: number;
  title: string;
  infinitasTitle: string | null;
  difficulty: string;
  tier: string;
  attributes: string | null;
  sortOrder: number | null;
}

interface LampData {
  lamp: string;
  exScore: number | null;
  missCount: number | null;
}

export interface TableEntry {
  id: number;
  title: string;
  infinitasTitle: string | null;
  difficulty: string;
  attributes: string | null;
  lamp: string;
  exScore: number | null;
  missCount: number | null;
}

export interface TierGroup {
  tier: string;
  entries: TableEntry[];
}

export function buildLampMap(
  userLamps: Array<{
    infinitasTitle: string;
    difficulty: string;
    lamp: string;
    exScore: number | null;
    missCount: number | null;
  }>,
): Map<string, LampData> {
  const lampMap = new Map<string, LampData>();
  for (const l of userLamps) {
    lampMap.set(`${l.infinitasTitle}:${l.difficulty}`, {
      lamp: l.lamp,
      exScore: l.exScore,
      missCount: l.missCount,
    });
  }
  return lampMap;
}

const TIER_ORDER: string[] = [
  "地力S+", "個人差S+",
  "地力S", "個人差S",
  "地力A+", "個人差A+",
  "地力A", "個人差A",
  "地力B+", "個人差B+",
  "地力B", "個人差B",
  "地力C", "個人差C",
  "地力D", "個人差D",
  "地力E", "個人差E",
  "地力F", "個人差F",
  "超個人差",
  "未定",
];

const LAMP_TYPE_ORDER = ["normal", "hard"] as const;

export function formatTableKey(tableKey: string): string {
  const match = tableKey.match(/^(sp|dp)(\d+)-(normal|hard)$/);
  if (!match) {
    return tableKey;
  }
  const [, playStyle = "", level = "", lampType = ""] = match;
  return `${playStyle.toUpperCase()}☆${level} ${lampType.toUpperCase()}`;
}

export function sortTableKeys<T extends { tableKey: string }>(
  keys: T[],
): T[] {
  return [...keys].sort((a, b) => {
    const ma = a.tableKey.match(/^(sp|dp)(\d+)-(normal|hard)$/);
    const mb = b.tableKey.match(/^(sp|dp)(\d+)-(normal|hard)$/);
    if (!ma || !mb) {
      return 0;
    }
    const levelDiff = Number(ma[2]) - Number(mb[2]);
    if (levelDiff !== 0) {
      return levelDiff;
    }
    return (
      LAMP_TYPE_ORDER.indexOf(ma[3] as (typeof LAMP_TYPE_ORDER)[number]) -
      LAMP_TYPE_ORDER.indexOf(mb[3] as (typeof LAMP_TYPE_ORDER)[number])
    );
  });
}

export function groupChartsByTier(
  chartRows: ChartRow[],
  lampMap: Map<string, LampData>,
): TierGroup[] {
  const tierMap = new Map<string, TableEntry[]>();

  for (const chart of chartRows) {
    const key = `${chart.infinitasTitle ?? chart.title}:${chart.difficulty}`;
    const lampData = lampMap.get(key);

    const entry: TableEntry = {
      id: chart.id,
      title: chart.title,
      infinitasTitle: chart.infinitasTitle,
      difficulty: chart.difficulty,
      attributes: chart.attributes,
      lamp: lampData?.lamp ?? "NO PLAY",
      exScore: lampData?.exScore ?? null,
      missCount: lampData?.missCount ?? null,
    };

    const tierEntries = tierMap.get(chart.tier);
    if (tierEntries) {
      tierEntries.push(entry);
    } else {
      tierMap.set(chart.tier, [entry]);
    }
  }

  const unknownTierOrder = TIER_ORDER.length - 3;

  return Array.from(tierMap.entries())
    .map(([tier, entries]) => ({ tier, entries }))
    .sort((a, b) => {
      const ai = TIER_ORDER.indexOf(a.tier);
      const bi = TIER_ORDER.indexOf(b.tier);
      return (ai === -1 ? unknownTierOrder : ai) - (bi === -1 ? unknownTierOrder : bi);
    });
}
