import fsd from "@feature-sliced/steiger-plugin";
import { defineConfig } from "steiger";

export default defineConfig([
  ...fsd.configs.recommended,
  {
    ignores: ["**/wasm-pkg/**", "**/shared/lib/types/generated.ts"],
  },
  {
    // `insignificant-slice` flags any slice with a single consumer. In this app
    // the studio page is the composition root, so nearly every widget/feature is
    // single-consumer by design — collapsing them would destroy the FSD layers.
    // All other FSD rules (public-api, no-public-api-sidestep, segments-by-purpose,
    // inconsistent-naming, forbidden-imports) are enforced.
    rules: {
      "fsd/insignificant-slice": "off",
    },
  },
]);
