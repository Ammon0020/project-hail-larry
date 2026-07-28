/**
 * Shared CodeMirror language resolution — used by the editor pane and the
 * read-only git diff viewer so the two stay in sync.
 *
 * `languageExtensionsForPath` is synchronous: it returns the built-in
 * language extensions for known extensions (js/ts/css/html/python/markdown/
 * json) and any already-loaded `@codemirror/language-data` support for
 * others (rust, go, yaml, …). It returns `[]` for unknown languages or
 * not-yet-loaded language-data entries. Callers that need lazy loading
 * (e.g. {@link EditorPane}) use {@link languageDescriptionForPath} to
 * trigger a load when this returns `[]` for a known-but-unloaded language.
 */

import { javascript } from '@codemirror/lang-javascript'
import { css } from '@codemirror/lang-css'
import { html } from '@codemirror/lang-html'
import { python } from '@codemirror/lang-python'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { json } from '@codemirror/lang-json'
import { languages as mdLanguages } from '@codemirror/language-data'
import { LanguageDescription } from '@codemirror/language'
import type { Extension } from '@codemirror/state'

/** Lowercased file extension (without the dot), or `''` for no extension. */
function extOf(path: string): string {
  const filename = basenameOf(path)
  const dot = filename.lastIndexOf('.')
  if (dot <= 0) return ''
  return filename.slice(dot + 1).toLowerCase()
}

/** Basename of a path (last segment after any `/` or `\`). */
function basenameOf(path: string): string {
  const idx = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return idx < 0 ? path : path.slice(idx + 1)
}

/**
 * Resolve CodeMirror language extensions for a file path, synchronously.
 *
 * Returns `[]` when the language is unknown or not yet loaded; callers that
 * need highlighting in that case should trigger a lazy load via
 * {@link languageDescriptionForPath} and re-render.
 */
export function languageExtensionsForPath(path: string): Extension[] {
  const ext = extOf(path)
  if (['javascript', 'js', 'jsx', 'ts', 'tsx', 'mjs', 'cjs'].includes(ext)) {
    return [
      javascript({
        jsx: ext === 'jsx' || ext === 'tsx',
        typescript: ext === 'ts' || ext === 'tsx',
      }),
    ]
  }
  if (['css', 'scss', 'less'].includes(ext)) return [css()]
  if (['html', 'htm', 'xml', 'svg'].includes(ext)) return [html()]
  if (['python', 'py', 'pyw'].includes(ext)) return [python()]
  if (['markdown', 'md', 'mdx', 'mdown'].includes(ext)) {
    // markdown() provides structure highlighting; markdownLanguage +
    // mdLanguages enables nested code-block highlighting (```js, ```python)
    // via lazy language loading.
    return [markdown({ base: markdownLanguage, codeLanguages: mdLanguages })]
  }
  if (['json', 'jsonc'].includes(ext)) return [json()]
  // Fall back to @codemirror/language-data filename matching for anything
  // not handled above (rust, go, yaml, shell, …). Synchronous when the
  // language is already loaded; otherwise returns [].
  const desc = languageDescriptionForPath(path)
  if (desc?.support) return [desc.support]
  return []
}

/**
 * Look up the (possibly unloaded) `LanguageDescription` for a path via
 * `@codemirror/language-data` filename matching. Returns `null` when no
 * language-data entry matches. Used by callers that need to trigger lazy
 * loading when {@link languageExtensionsForPath} returns `[]`.
 */
export function languageDescriptionForPath(path: string): LanguageDescription | null {
  return LanguageDescription.matchFilename(mdLanguages, basenameOf(path))
}
