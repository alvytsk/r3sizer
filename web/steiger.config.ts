import fsd from "@feature-sliced/steiger-plugin";
import { defineConfig } from "steiger";

export default defineConfig([
  ...fsd.configs.recommended,
  {
    ignores: ["**/wasm-pkg/**", "**/shared/types/generated.ts"],
  },
  {
    // These structural/naming rules are disabled to preserve the scope of the
    // previous hand-rolled ESLint rule, which enforced only import direction and
    // cross-slice isolation (the `fsd/forbidden-imports` rule, kept enabled).
    // Re-enable any of them individually to adopt more FSD conventions.
    rules: {
      "fsd/no-public-api-sidestep": "off",
      "fsd/public-api": "off",
      "fsd/insignificant-slice": "off",
      "fsd/segments-by-purpose": "off",
      "fsd/inconsistent-naming": "off",
    },
  },
]);
