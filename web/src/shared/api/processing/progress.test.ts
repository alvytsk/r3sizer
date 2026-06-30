import { describe, expect, it } from "vitest";
import { ProgressAggregator, type ProgressEvent } from "./progress";

describe("ProgressAggregator", () => {
  it("normalizes overall progress over the active stages", () => {
    const events: ProgressEvent[] = [];
    const agg = new ProgressAggregator(["prepare", "probe", "finalize"], (e) => events.push(e));
    agg.update("prepare", 1);
    agg.update("probe", 0.5);
    agg.complete("finalize");

    expect(events[0].overall).toBeGreaterThan(0);
    expect(events.at(-1)!.overall).toBeCloseTo(1, 5);
    // Monotonically non-decreasing.
    for (let i = 1; i < events.length; i++) {
      expect(events[i].overall).toBeGreaterThanOrEqual(events[i - 1].overall);
    }
  });

  it("marks earlier stages complete when a later stage reports", () => {
    const events: ProgressEvent[] = [];
    const agg = new ProgressAggregator(["ingest", "prepare", "probe", "finalize"], (e) =>
      events.push(e),
    );
    // Jump straight to probe: ingest + prepare count as done.
    agg.update("probe", 0);
    const overallAtProbeStart = events.at(-1)!.overall;
    agg.update("ingest", 0.1); // late/out-of-order event must not regress
    expect(events.at(-1)!.overall).toBeGreaterThanOrEqual(overallAtProbeStart);
  });

  it("clamps fractions to [0, 1]", () => {
    const events: ProgressEvent[] = [];
    const agg = new ProgressAggregator(["prepare", "probe"], (e) => events.push(e));
    agg.update("prepare", 7);
    expect(events.at(-1)!.fraction).toBe(1);
    agg.update("probe", -2);
    expect(events.at(-1)!.fraction).toBe(0);
  });
});
