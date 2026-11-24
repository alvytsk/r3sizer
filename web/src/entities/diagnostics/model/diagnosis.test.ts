import i18next from "i18next";
import { expect, it } from "vitest";
import type { AutoSharpDiagnostics, RobustnessFlags } from "@/shared/lib";
import en from "../../../../public/locales/en.json";
import ru from "../../../../public/locales/ru.json";
import { buildDiagnosis } from "./diagnosis";

function makeT(lng: "en" | "ru" = "en") {
  const instance = i18next.createInstance();
  instance.init({
    lng,
    fallbackLng: "en",
    initAsync: false,
    resources: { en: { translation: en }, ru: { translation: ru } },
  });
  return instance.t;
}

function looEntries(robustness: Partial<RobustnessFlags>, lng: "en" | "ru" = "en") {
  const d = {
    selection_mode: "polynomial_root",
    fallback_reason: null,
    baseline_artifact_ratio: 0,
    robustness: {
      monotonic: true,
      quasi_monotonic: true,
      r_squared_ok: true,
      well_conditioned: true,
      loo_stable: true,
      max_loo_root_change: 0,
      loo_coverage: 1,
      ...robustness,
    },
  } as unknown as AutoSharpDiagnostics;
  return buildDiagnosis(d, makeT(lng)).filter(
    (e) => e.headline.startsWith("Robustness: LOO") || e.headline.startsWith("Устойчивость: LOO"),
  );
}

it("explains an unstable LOO root that stays in range by its shift", () => {
  const [loo] = looEntries({ loo_stable: false, max_loo_root_change: 0.312, loo_coverage: 1 });

  expect(loo?.headline).toBe("Robustness: LOO unstable (Δs* = 0.312)");
});

it("explains an unstable LOO root that left the range by its coverage", () => {
  // A 4% shift is below the 25% threshold: the shift alone cannot explain
  // why the check failed.
  const [loo] = looEntries({
    loo_stable: false,
    max_loo_root_change: 0.0399,
    loo_coverage: 6 / 7,
  });

  expect(loo?.headline).toBe("Robustness: LOO unstable (root in range: 86% of refits)");
  expect(loo?.detail).not.toContain("shifts the root");
});

it("names the coverage in Russian too", () => {
  const [loo] = looEntries(
    { loo_stable: false, max_loo_root_change: 0.0399, loo_coverage: 6 / 7 },
    "ru",
  );

  expect(loo?.headline).toBe(
    "Устойчивость: LOO нестабилен (корень в\u00a0диапазоне: 86% повторных подборов)",
  );
});

it("reports no LOO entry when the check passes or does not run", () => {
  expect(looEntries({ loo_stable: true, max_loo_root_change: 0, loo_coverage: 1 })).toEqual([]);
});
