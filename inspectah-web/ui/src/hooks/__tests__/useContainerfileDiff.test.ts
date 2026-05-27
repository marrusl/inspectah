import { describe, it, expect, beforeEach } from "vitest";
import {
  computeDiff,
  _resetIdCounter,
  type DiffLine,
  type DiffResult,
} from "../useContainerfileDiff";

beforeEach(() => {
  _resetIdCounter();
});

describe("computeDiff", () => {
  it("returns all stable lines when strings are identical", () => {
    const result = computeDiff("FROM ubi9\nRUN dnf install -y httpd\n", "FROM ubi9\nRUN dnf install -y httpd\n");
    expect(result.lines).toHaveLength(2);
    expect(result.lines.every((l) => l.state === "stable")).toBe(true);
    expect(result.addedCount).toBe(0);
    expect(result.removedCount).toBe(0);
    expect(result.hasChanges).toBe(false);
  });

  it("marks added lines when new content has extra lines", () => {
    const prev = "FROM ubi9\n";
    const next = "FROM ubi9\nRUN dnf install -y httpd\n";
    const result = computeDiff(prev, next);
    expect(result.lines).toHaveLength(2);
    expect(result.lines[0]).toMatchObject({ text: "FROM ubi9", state: "stable" });
    expect(result.lines[1]).toMatchObject({ text: "RUN dnf install -y httpd", state: "added" });
    expect(result.addedCount).toBe(1);
    expect(result.removedCount).toBe(0);
    expect(result.hasChanges).toBe(true);
  });

  it("marks removed lines when content has fewer lines", () => {
    const prev = "FROM ubi9\nRUN dnf install -y httpd\n";
    const next = "FROM ubi9\n";
    const result = computeDiff(prev, next);
    expect(result.lines).toHaveLength(2);
    expect(result.lines[0]).toMatchObject({ text: "FROM ubi9", state: "stable" });
    expect(result.lines[1]).toMatchObject({ text: "RUN dnf install -y httpd", state: "removing" });
    expect(result.addedCount).toBe(0);
    expect(result.removedCount).toBe(1);
    expect(result.hasChanges).toBe(true);
  });

  it("handles simultaneous adds and removes", () => {
    const prev = "FROM ubi9\nRUN dnf install -y httpd\n";
    const next = "FROM ubi9\nRUN dnf install -y nginx\n";
    const result = computeDiff(prev, next);
    const texts = result.lines.map((l) => l.text);
    expect(texts).toContain("FROM ubi9");
    expect(texts).toContain("RUN dnf install -y httpd");
    expect(texts).toContain("RUN dnf install -y nginx");
    expect(result.lines.find((l) => l.text === "FROM ubi9")!.state).toBe("stable");
    expect(result.lines.find((l) => l.text === "RUN dnf install -y httpd")!.state).toBe("removing");
    expect(result.lines.find((l) => l.text === "RUN dnf install -y nginx")!.state).toBe("added");
    expect(result.addedCount).toBe(1);
    expect(result.removedCount).toBe(1);
    expect(result.hasChanges).toBe(true);
  });

  it("handles duplicate lines with unique IDs", () => {
    const content = "ENV FOO=bar\nENV FOO=bar\n";
    const result = computeDiff(content, content);
    expect(result.lines).toHaveLength(2);
    expect(result.lines[0].id).not.toBe(result.lines[1].id);
    expect(result.lines[0].text).toBe("ENV FOO=bar");
    expect(result.lines[1].text).toBe("ENV FOO=bar");
  });

  it("preserves IDs for unchanged lines across successive diffs", () => {
    const first = computeDiff("FROM ubi9\nRUN echo hello\n", "FROM ubi9\nRUN echo hello\nCOPY . /app\n");
    const fromFirstId = first.lines.find((l) => l.text === "FROM ubi9")!.id;
    const runFirstId = first.lines.find((l) => l.text === "RUN echo hello")!.id;

    const second = computeDiff(
      "FROM ubi9\nRUN echo hello\nCOPY . /app\n",
      "FROM ubi9\nRUN echo hello\nCOPY . /app\nEXPOSE 8080\n",
      first.lines,
    );
    expect(second.lines.find((l) => l.text === "FROM ubi9")!.id).toBe(fromFirstId);
    expect(second.lines.find((l) => l.text === "RUN echo hello")!.id).toBe(runFirstId);
  });

  it("preserves IDs for unchanged duplicate lines across successive diffs", () => {
    const content = "ENV FOO=bar\nRUN echo\nENV FOO=bar\n";
    const first = computeDiff(content, content);
    const firstIds = first.lines.filter((l) => l.text === "ENV FOO=bar").map((l) => l.id);
    expect(firstIds).toHaveLength(2);
    expect(firstIds[0]).not.toBe(firstIds[1]);

    const second = computeDiff(content, content, first.lines);
    const secondIds = second.lines.filter((l) => l.text === "ENV FOO=bar").map((l) => l.id);
    expect(secondIds).toEqual(firstIds);
  });

  it("preserves IDs across three successive diffs", () => {
    const v1 = "FROM ubi9\nRUN echo a\n";
    const v2 = "FROM ubi9\nRUN echo a\nRUN echo b\n";
    const v3 = "FROM ubi9\nRUN echo a\nRUN echo b\nRUN echo c\n";

    const d1 = computeDiff(v1, v2);
    const fromId = d1.lines.find((l) => l.text === "FROM ubi9")!.id;
    const echoAId = d1.lines.find((l) => l.text === "RUN echo a")!.id;

    const d2 = computeDiff(v2, v3, d1.lines);
    expect(d2.lines.find((l) => l.text === "FROM ubi9")!.id).toBe(fromId);
    expect(d2.lines.find((l) => l.text === "RUN echo a")!.id).toBe(echoAId);
    const echoBId = d2.lines.find((l) => l.text === "RUN echo b")!.id;

    const v4 = "FROM ubi9\nRUN echo a\nRUN echo b\nRUN echo c\nRUN echo d\n";
    const d3 = computeDiff(v3, v4, d2.lines);
    expect(d3.lines.find((l) => l.text === "FROM ubi9")!.id).toBe(fromId);
    expect(d3.lines.find((l) => l.text === "RUN echo a")!.id).toBe(echoAId);
    expect(d3.lines.find((l) => l.text === "RUN echo b")!.id).toBe(echoBId);
  });

  it("preserves surviving duplicate ID when one duplicate is removed", () => {
    const v1 = "ENV X=1\nENV X=1\n";
    const v2 = "ENV X=1\n";

    const d1 = computeDiff(v1, v1);
    const [id0, id1] = d1.lines.map((l) => l.id);

    // diffLines keeps the first occurrence stable and removes the second.
    // The surviving (unchanged) line shifts id0 from the FIFO; the removed
    // line shifts id1. So the surviving line gets id0.
    const d2 = computeDiff(v1, v2, d1.lines);
    const surviving = d2.lines.find((l) => l.state === "stable");
    expect(surviving).toBeDefined();
    expect(surviving!.id).toBe(id0);
    // The removed line consumed id1
    const removed = d2.lines.find((l) => l.state === "removing");
    expect(removed).toBeDefined();
    expect(removed!.id).toBe(id1);
  });

  it("preserves correct ID for non-adjacent surviving duplicate", () => {
    // a, b, a → b, a
    // The first 'a' is removed, the second 'a' survives.
    // The surviving 'a' should get the SECOND occurrence's ID, not the first's.
    const v1 = "a\nb\na\n";
    const v2 = "b\na\n";

    const d1 = computeDiff(v1, v1);
    const aLines = d1.lines.filter((l) => l.text === "a");
    expect(aLines).toHaveLength(2);
    const [aId0, aId1] = aLines.map((l) => l.id);
    const bId = d1.lines.find((l) => l.text === "b")!.id;

    const d2 = computeDiff(v1, v2, d1.lines);
    // b should keep its ID
    expect(d2.lines.find((l) => l.text === "b" && l.state === "stable")!.id).toBe(bId);
    // Surviving 'a' should get the second occurrence's ID
    const survivingA = d2.lines.find((l) => l.text === "a" && l.state === "stable");
    expect(survivingA).toBeDefined();
    expect(survivingA!.id).toBe(aId1);
  });

  it("settles added lines to stable with preserved ID on next diff", () => {
    const v1 = "FROM ubi9\n";
    const v2 = "FROM ubi9\nRUN echo hello\n";

    const d1 = computeDiff(v1, v2);
    const addedLine = d1.lines.find((l) => l.state === "added");
    expect(addedLine).toBeDefined();
    const addedId = addedLine!.id;

    // On next diff with same content, the added line should settle to stable
    const d2 = computeDiff(v2, v2, d1.lines);
    const settled = d2.lines.find((l) => l.text === "RUN echo hello");
    expect(settled).toBeDefined();
    expect(settled!.state).toBe("stable");
    expect(settled!.id).toBe(addedId);
  });

  it("returns baseline (all stable) when prev is null", () => {
    const result = computeDiff(null, "FROM ubi9\nRUN echo hello\n");
    expect(result.lines).toHaveLength(2);
    expect(result.lines.every((l) => l.state === "stable")).toBe(true);
    expect(result.hasChanges).toBe(false);
  });

  it("returns empty when both are null", () => {
    const result = computeDiff(null, null);
    expect(result.lines).toHaveLength(0);
    expect(result.hasChanges).toBe(false);
  });

  it("handles entire section appearing", () => {
    const prev = "FROM ubi9\n";
    const next = "FROM ubi9\nRUN dnf install -y httpd\nRUN dnf install -y nginx\nEXPOSE 80\nEXPOSE 443\n";
    const result = computeDiff(prev, next);
    expect(result.lines).toHaveLength(5);
    expect(result.lines[0]).toMatchObject({ text: "FROM ubi9", state: "stable" });
    expect(result.lines.filter((l) => l.state === "added")).toHaveLength(4);
    expect(result.addedCount).toBe(4);
    expect(result.removedCount).toBe(0);
    expect(result.hasChanges).toBe(true);
    // All IDs should be unique
    const ids = result.lines.map((l) => l.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
