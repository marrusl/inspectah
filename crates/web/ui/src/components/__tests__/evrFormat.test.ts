import { describe, it, expect } from "vitest";
import { formatEvrPair } from "../evrFormat";

describe("formatEvrPair", () => {
  it("omits epoch when both sides are 0 or empty", () => {
    expect(formatEvrPair("", "2.4.51", "", "2.4.57")).toEqual([
      "2.4.51",
      "2.4.57",
    ]);
    expect(formatEvrPair("0", "2.4.51", "0", "2.4.57")).toEqual([
      "2.4.51",
      "2.4.57",
    ]);
  });

  it("shows epoch on both sides when either has non-zero epoch", () => {
    expect(formatEvrPair("1", "2.4.51", "0", "2.4.57")).toEqual([
      "1:2.4.51",
      "0:2.4.57",
    ]);
    expect(formatEvrPair("0", "2.4.51", "1", "2.4.57")).toEqual([
      "0:2.4.51",
      "1:2.4.57",
    ]);
  });

  it("shows epoch on both sides when epochs differ even if both non-zero", () => {
    expect(formatEvrPair("1", "2.4.51", "2", "2.4.57")).toEqual([
      "1:2.4.51",
      "2:2.4.57",
    ]);
  });

  it("shows epoch on both sides when one is non-zero and other is empty", () => {
    expect(formatEvrPair("1", "2.4.51", "", "2.4.57")).toEqual([
      "1:2.4.51",
      "0:2.4.57",
    ]);
  });
});
