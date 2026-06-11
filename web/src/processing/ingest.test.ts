import { describe, expect, it } from "vitest";
import { planStripes } from "./ingest";

describe("planStripes", () => {
  it("targets ~16MB stripes for a wide panorama", () => {
    // 40000 px wide -> 160000 bytes/row -> 16MB/row-bytes = 104 rows.
    const plan = planStripes(40000, 2500);
    expect(plan.stripeHeight).toBe(104);
    expect(plan.count).toBe(Math.ceil(2500 / 104));
  });

  it("clamps stripe height to at least 16 rows", () => {
    // Absurdly wide image: 16MB / (2_000_000 * 4) = 2 rows -> clamped to 16.
    expect(planStripes(2_000_000, 100).stripeHeight).toBe(16);
  });

  it("clamps stripe height to at most 1024 rows", () => {
    // Narrow image: 16MB / (100 * 4) = 41943 rows -> clamped to 1024.
    expect(planStripes(100, 50_000).stripeHeight).toBe(1024);
  });

  it("covers every row exactly once", () => {
    const { stripeHeight, count } = planStripes(8000, 12500);
    expect((count - 1) * stripeHeight).toBeLessThan(12500);
    expect(count * stripeHeight).toBeGreaterThanOrEqual(12500);
  });
});
