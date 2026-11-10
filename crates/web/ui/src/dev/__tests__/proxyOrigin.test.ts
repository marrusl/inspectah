import { describe, it, expect } from "vitest";
import { proxiedOrigin, REFINE_ORIGIN } from "../proxyOrigin";

describe("proxiedOrigin", () => {
  it.each([
    ["http://localhost:5174", "localhost:5174"],
    ["http://127.0.0.1:5173", "127.0.0.1:5173"],
    ["http://[::1]:5174", "[::1]:5174"],
  ])("rewrites the dev page's own origin %s to the refine origin", (origin, host) => {
    expect(proxiedOrigin(origin, host)).toBe(REFINE_ORIGIN);
  });

  it.each([
    // A foreign page posting through the dev server.
    ["http://attacker.example", "localhost:5174"],
    // A rebinding page: its origin matches Host, but Host is not loopback.
    ["http://attacker.example:5174", "attacker.example:5174"],
    // Loopback, but another local service, not this dev server.
    ["http://localhost:3000", "localhost:5174"],
    ["null", "localhost:5174"],
    ["http://localhost.attacker.example:5174", "localhost.attacker.example:5174"],
  ])("passes a foreign origin %s through unchanged", (origin, host) => {
    expect(proxiedOrigin(origin, host)).toBe(origin);
  });

  it("leaves a request without an Origin alone", () => {
    expect(proxiedOrigin(undefined, "localhost:5174")).toBeUndefined();
  });
});
