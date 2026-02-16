interface ChartRow {
  id: number;
  title: string;
  infinitasTitle: string | null;
  difficulty: string;
  tier: string;
  attributes: string | null;
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

  return Array.from(tierMap.entries()).map(([tier, entries]) => ({
    tier,
    entries,
  }));
}
