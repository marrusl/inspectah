import { describe, it, expect } from "vitest";
import { lockedReasonLabel } from "../attentionUtils";

describe("lockedReasonLabel (§ 9.3, § 7 string 24)", () => {
  it("names the image-mode lock in words", () => {
    expect(lockedReasonLabel("service-image-mode-incompatible")).toBe("Incompatible with image mode");
  });

  it.each([null, undefined])("is LOCKED for a %s reason", (reason) => {
    expect(lockedReasonLabel(reason)).toBe("LOCKED");
  });

  it("gives any other reason as it is", () => {
    expect(lockedReasonLabel("held by the base image")).toBe("held by the base image");
  });
});
