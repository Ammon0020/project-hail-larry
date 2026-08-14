import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import eslintPluginTailwindcss from 'eslint-plugin-tailwindcss'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
  },
  {
    // Tailwind CSS v4 linting — class order, contradictions, arbitrary
    // values, and unknown classnames. Uses the CSS-first config (no
    // tailwind.config.js) via cssConfigPath pointing at the main stylesheet.
    files: ['**/*.{ts,tsx}'],
    extends: [eslintPluginTailwindcss.configs.recommended],
    settings: {
      tailwindcss: {
        cssConfigPath: './src/index.css',
      },
    },
    rules: {
      // Start with warnings so existing code can be cleaned up gradually
      // without blocking CI. Promote to errors once the codebase is clean.
      'tailwindcss/classnames-order': 'warn',
      'tailwindcss/no-arbitrary-value': 'off',
      'tailwindcss/no-custom-classname': [
        'warn',
        {
          // Custom CSS classes used alongside Tailwind utilities — defined
          // in index.css or vendored assistant-ui stylesheets.
          whitelist: [
            'aui-.*',
            'bg-checkerboard',
            'prose-docx',
            'hide-scrollbar',
            'tab-scrollbar',
            'session-item',
            'select-chevron',
            'inputs',
          ],
        },
      ],
      'tailwindcss/no-contradicting-classname': 'warn',
    },
  },
  {
    // shadcn/ui primitives, assistant-ui registry components, and lib
    // utilities intentionally export helpers (e.g. buttonVariants from cva,
    // fileIcon lookup, reasoningVariants) alongside components. That is a
    // shared convention for these vendored building blocks, so the
    // react-refresh "only export components" rule (a Fast-Refresh ergonomics
    // hint) does not apply here. The react-hooks refs/set-state rules are also
    // relaxed for vendored registry code we don't maintain.
    files: [
      'src/components/ui/**/*.{ts,tsx}',
      'src/components/assistant-ui/**/*.{ts,tsx}',
      'src/lib/**/*.{ts,tsx}',
    ],
    rules: {
      'react-refresh/only-export-components': 'off',
      'react-hooks/refs': 'off',
      'react-hooks/set-state-in-effect': 'off',
      // Vendored assistant-ui uses non-standard variant syntax (e.g.
      // active:scale-0.98, stroke-1.5) that the plugin doesn't recognize.
      'tailwindcss/no-custom-classname': 'off',
    },
  },
])
