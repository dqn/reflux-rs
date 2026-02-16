// Lamp values matching the Rust Lamp enum ordering
export const LAMP_VALUES = [
  "NO PLAY",
  "FAILED",
  "ASSIST",
  "EASY",
  "CLEAR",
  "HARD",
  "EX HARD",
  "FC",
] as const;

export type LampValue = (typeof LAMP_VALUES)[number];

const LAMP_ORDER: Record<LampValue, number> = {
  "NO PLAY": 0,
  FAILED: 1,
  ASSIST: 2,
  EASY: 3,
  CLEAR: 4,
  HARD: 5,
  "EX HARD": 6,
  FC: 7,
};

export interface LampStyle {
  background: string;
  color: string;
  border?: string;
}

const LAMP_STYLES: Record<LampValue, LampStyle> = {
  "NO PLAY": { background: "#333", color: "#888" },
  FAILED: { background: "#c0392b", color: "#fff" },
  ASSIST: { background: "#8e6cbf", color: "#fff" },
  EASY: { background: "#6abf40", color: "#000" },
  CLEAR: { background: "#3db8c9", color: "#000" },
  HARD: { background: "#e0e0e0", color: "#111", border: "1px solid #555" },
  "EX HARD": { background: "#d4aa00", color: "#000" },
  FC: { background: "#3db8c9", color: "#000" },
};

export function getLampOrder(lamp: string): number {
  return LAMP_ORDER[lamp as LampValue] ?? -1;
}

export function isHigherLamp(newLamp: string, currentLamp: string): boolean {
  return getLampOrder(newLamp) > getLampOrder(currentLamp);
}

export function getLampStyle(lamp: string): LampStyle {
  return LAMP_STYLES[lamp as LampValue] ?? LAMP_STYLES["NO PLAY"];
}

export function isValidLamp(lamp: string): lamp is LampValue {
  return lamp in LAMP_ORDER;
}
