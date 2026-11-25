import { beforeEach, expect, it, vi } from "vitest";

vi.mock("./wasm-pkg/r3sizer_wasm", () => ({
  initSync: vi.fn(),
  probe_batch_with_detail: vi.fn(),
}));

import { type BaseData, runProbeBatch } from "./probe-worker";
import { probe_batch_with_detail } from "./wasm-pkg/r3sizer_wasm";

const base: BaseData = {
  basePixels: new Float32Array(3),
  luminance: new Float32Array(1),
  width: 1,
  height: 1,
  baseline: 0.25,
  effectiveP0: 0.5,
  detail: new Float32Array(1),
  paramsJson: '{"target_width":1}',
  provenanceJson: '{"base_id":7}',
};

beforeEach(() => vi.mocked(probe_batch_with_detail).mockReset());

// In Lightness mode `luminance` and `detail` are both W×H floats, so a swap at
// this call site would compile and run. The binding's order is the contract.
it("passes the cached base to wasm in the binding's argument order", () => {
  vi.mocked(probe_batch_with_detail).mockReturnValueOnce('{"provenance":{},"samples":[]}');
  expect(runProbeBatch(base, [0.5, 1])).toBe('{"provenance":{},"samples":[]}');
  expect(vi.mocked(probe_batch_with_detail).mock.calls[0]).toEqual([
    base.basePixels,
    1,
    1,
    base.luminance,
    base.detail,
    "[0.5,1]",
    base.paramsJson,
    0.25,
    base.provenanceJson,
  ]);
});
