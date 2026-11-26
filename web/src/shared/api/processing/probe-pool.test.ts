import { describe, expect, it } from "vitest";
import { mergeProbeBatches } from "./probe-pool";

const stamp = {
  base_id: 3,
  baseline_artifact_ratio_bits: 1050253722,
  settings: { sharpen_sigma_bits: 1065353216 },
};
const sample = (strength: number) => ({ strength, artifact_ratio: 0.1, metric_value: 0.1 });
const batch = (provenance: unknown, samples: unknown[]) => JSON.stringify({ provenance, samples });

describe("mergeProbeBatches", () => {
  it("keeps one stamp and sorts and deduplicates the samples", () => {
    const merged = JSON.parse(
      mergeProbeBatches([
        batch(stamp, [sample(1), sample(0.5)]),
        batch(stamp, [sample(1 + 1e-6), sample(2)]),
      ]),
    );
    expect(merged.provenance).toEqual(stamp);
    expect(merged.samples.map((s: { strength: number }) => s.strength)).toEqual([0.5, 1, 2]);
  });

  it("refuses batches that disagree on the stamp", () => {
    expect(() =>
      mergeProbeBatches([batch(stamp, [sample(1)]), batch({ ...stamp, base_id: 4 }, [sample(2)])]),
    ).toThrow(/disagree on provenance/);
  });

  it("refuses an empty round", () => {
    expect(() => mergeProbeBatches([])).toThrow(/no probe batches to merge/);
  });
});
