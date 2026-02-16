import { describe, it, expect } from "vitest";

import {
  getLampOrder,
  isHigherLamp,
  getLampStyle,
  isValidLamp,
  LAMP_VALUES,
} from "../../lib/lamp";

describe("getLampOrder", () => {
  it("returns correct order for valid lamps", () => {
    expect(getLampOrder("NO PLAY")).toBe(0);
    expect(getLampOrder("FAILED")).toBe(1);
    expect(getLampOrder("ASSIST")).toBe(2);
    expect(getLampOrder("EASY")).toBe(3);
    expect(getLampOrder("CLEAR")).toBe(4);
    expect(getLampOrder("HARD")).toBe(5);
    expect(getLampOrder("EX HARD")).toBe(6);
    expect(getLampOrder("FC")).toBe(7);
    expect(getLampOrder("PFC")).toBe(8);
  });

  it("returns -1 for invalid lamp", () => {
    expect(getLampOrder("INVALID")).toBe(-1);
    expect(getLampOrder("")).toBe(-1);
  });
});

describe("isHigherLamp", () => {
  it("returns true when new lamp is higher", () => {
    expect(isHigherLamp("CLEAR", "NO PLAY")).toBe(true);
    expect(isHigherLamp("HARD", "CLEAR")).toBe(true);
    expect(isHigherLamp("PFC", "FC")).toBe(true);
  });

  it("returns false when new lamp is equal", () => {
    expect(isHigherLamp("CLEAR", "CLEAR")).toBe(false);
  });

  it("returns false when new lamp is lower", () => {
    expect(isHigherLamp("NO PLAY", "CLEAR")).toBe(false);
    expect(isHigherLamp("EASY", "HARD")).toBe(false);
  });
});

describe("getLampStyle", () => {
  it("returns style for valid lamps", () => {
    const style = getLampStyle("HARD");
    expect(style.background).toBe("#e0e0e0");
    expect(style.color).toBe("#111");
    expect(style.border).toBe("1px solid #555");
  });

  it("returns NO PLAY style for invalid lamp", () => {
    const style = getLampStyle("INVALID");
    expect(style).toEqual(getLampStyle("NO PLAY"));
  });
});

describe("isValidLamp", () => {
  it("returns true for all valid lamp values", () => {
    for (const lamp of LAMP_VALUES) {
      expect(isValidLamp(lamp)).toBe(true);
    }
  });

  it("returns false for invalid values", () => {
    expect(isValidLamp("INVALID")).toBe(false);
    expect(isValidLamp("")).toBe(false);
    expect(isValidLamp("clear")).toBe(false);
  });
});
