import { describe, expect, it } from "vitest";

describe("vitest setup", () => {
  it("runs in a DOM-like environment", () => {
    expect(typeof document).toBe("object");
  });
});
