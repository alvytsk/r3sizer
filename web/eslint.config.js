import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

/**
 * Feature-Sliced Design (v2.1) boundary enforcement.
 *
 * Layers, most-foundational first (a layer may only import from layers at or
 * below its own rank):
 *   shared(0) < entities(1) < features(2) < widgets(3) < pages(4) < app(5)
 *
 * Two rules:
 *  1. Direction — importing a HIGHER layer is forbidden
 *     (e.g. entities → features).
 *  2. Sibling slices — within a layer, one slice may not import another
 *     (e.g. features/export → features/image-processing). The `shared` and
 *     `app` layers are exempt: their internal segments may interoperate.
 *
 * Relative imports (intra-slice) and external/`@/`-aliased imports that do not
 * target an FSD layer are left unchecked.
 */
const LAYERS = { shared: 0, entities: 1, features: 2, widgets: 3, pages: 4, app: 5 }
const INTRA_LAYER_OK = new Set(['shared', 'app'])

const fsdLayerDirection = {
  meta: {
    type: 'problem',
    docs: { description: 'Enforce Feature-Sliced Design layer import direction' },
    schema: [],
  },
  create(context) {
    const filename = context.filename ?? ''
    const srcRel = filename.includes('/src/')
      ? filename.slice(filename.indexOf('/src/') + '/src/'.length)
      : ''
    const srcParts = srcRel.split('/')
    const sourceLayer = srcParts[0]
    const sourceSlice = srcParts[1]
    if (!(sourceLayer in LAYERS)) return {} // entry point / outside layers

    return {
      ImportDeclaration(node) {
        const spec = node.source.value
        if (typeof spec !== 'string' || !spec.startsWith('@/')) return
        const [, tgtLayer, tgtSlice] = spec.split('/')
        if (!(tgtLayer in LAYERS)) return

        const srcRank = LAYERS[sourceLayer]
        const tgtRank = LAYERS[tgtLayer]

        if (tgtRank > srcRank) {
          context.report({
            node,
            message:
              'FSD: layer "{{srcLayer}}" may not import higher layer "{{tgtLayer}}" ({{srcLayer}} → {{tgtLayer}}). Imports may only point to lower or equal layers.',
            data: { srcLayer: sourceLayer, tgtLayer },
          })
          return
        }

        if (tgtRank === srcRank && !INTRA_LAYER_OK.has(sourceLayer)) {
          if (tgtSlice && sourceSlice && tgtSlice !== sourceSlice) {
            context.report({
              node,
              message:
                'FSD: slice "{{srcSlice}}" may not import sibling slice "{{tgtSlice}}" within layer "{{layer}}" — cross-slice imports inside a layer are forbidden.',
              data: {
                srcSlice: sourceSlice,
                tgtSlice,
                layer: sourceLayer,
              },
            })
          }
        }
      },
    }
  },
}

const fsdPlugin = {
  meta: { name: 'fsd' },
  rules: { 'layer-direction': fsdLayerDirection },
}

export default defineConfig([
  globalIgnores([
    'dist',
    'src/shared/api/processing/wasm-pkg/**/*', // generated wasm-pack bindings
    'src/shared/types/generated.ts', // ts-rs generated
  ]),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
  },
  {
    // shadcn/ui primitives export cva variants alongside components;
    // HMR granularity is irrelevant for these vendored files.
    files: ['src/shared/ui/**/*.tsx'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },
  {
    files: ['src/**/*.{ts,tsx}'],
    plugins: { fsd: fsdPlugin },
    rules: { 'fsd/layer-direction': 'error' },
  },
])
