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
  "PFC",
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
  PFC: 8,
};

export interface LampStyle {
  background: string;
  color: string;
  border?: string;
}

const LAMP_STYLES: Record<LampValue, LampStyle> = {
  "NO PLAY": { background: "#666", color: "#fff" },
  FAILED: { background: "#e53e3e", color: "#fff" },
  ASSIST: { background: "#9f7aea", color: "#fff" },
  EASY: { background: "#80ff00", color: "#000" },
  CLEAR: { background: "#00e5ff", color: "#000" },
  HARD: { background: "#ffffff", color: "#000", border: "1px solid #666" },
  "EX HARD": { background: "#ffd700", color: "#000" },
  FC: { background: "#00e5ff", color: "#000" },
  PFC: {
    background: "linear-gradient(135deg, #ffd700, #ffec80, #ffd700)",
    color: "#000",
  },
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
