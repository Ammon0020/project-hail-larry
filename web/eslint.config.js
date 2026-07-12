import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
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
    // shadcn/ui primitives and lib utilities intentionally export helpers
    // (e.g. buttonVariants from cva, fileIcon lookup) alongside components.
    // That is a shared convention for these vendored building blocks, so the
    // react-refresh "only export components" rule (a Fast-Refresh ergonomics
    // hint) does not apply here.
    files: ['src/components/ui/**/*.{ts,tsx}', 'src/lib/**/*.{ts,tsx}'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },
])
