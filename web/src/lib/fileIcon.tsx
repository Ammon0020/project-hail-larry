/**
 * fileIcon — maps file names/extensions to VS Code Symbols-style SVG icons.
 *
 * Uses @react-symbols/icons (Miguel Solorio's Symbols icon theme) which
 * provides language-specific colored SVG glyphs — the same style VS Code
 * uses. Icons are imported individually so Vite tree-shakes unused ones.
 *
 * Used by the file tree (FileTree.tsx) and tab bar (TabBar.tsx).
 */
import {
  TypeScript, Tsconfig, TsTest,
  Js, Reactts, Reactjs,
  Go, GoMod, Python, Rust, Java, Kotlin, Swift,
  CLang, Cplus, Csharp, Ruby, PHP, Lua, Luau, Shell,
  Dart, Scala, Clojure, Haskell, Erlang, Elixir,
  Nim, Vlang, Zig, Solidity, R, Julia, Perl, Fortran,
  Fsharp, Rescript, RescriptInterface, Gleam, CoffeeScript,
  Graphql, Proto, Sass, Vue, Svelte, Astro,
  Next, Nuxt, Tailwind, PostCSS, Markdown, MDX,
  Yaml, XML, Csv, Image, SVG, Video, Audio, PDF,
  Document, Text, Lock, Ignore, License, Git, Docker,
  NPM, PNPM, Yarn, Vite, Vitest, Jest, Eslint, Prettier,
  Storybook, Prisma, Database, Gear, Zip, Puzzle, Exe,
  Font, Notebook, Vercel, Netlify, Stylelint, Biome,
  Webpack, Turborepo, CMake, Gradle, Shadcn,
  CodeOrange, CodeBlue, BracketsYellow,
  Http, Haml, Pug, Twig, Liquid, Coldfusion, Nunjucks,
  Stylus, Patch, I18n, EditorConfig, Knip, Cursor, Claude,
} from '@react-symbols/icons/files'
import { DefaultFileIcon } from '@react-symbols/icons/utils'
import { createElement, type ComponentType, type SVGProps } from 'react'

export type FileIconComponent = ComponentType<SVGProps<SVGSVGElement>>

/** Builds a record mapping each name to the same icon. Avoids repeating
 *  `{ icon: X }` for every entry — instead of `ts: { icon: TypeScript }`,
 *  we write `ts: TypeScript` and group shared icons in one call. */
function group(icon: FileIconComponent, ...names: string[]): Record<string, FileIconComponent> {
  return Object.fromEntries(names.map(n => [n, icon]))
}

// ---------------------------------------------------------------------------
// Extension → icon map
// ---------------------------------------------------------------------------

const EXT_MAP: Record<string, FileIconComponent> = {
  // TypeScript / JavaScript
  ...group(TypeScript, 'ts', 'mts', 'cts'),
  tsx: Reactts,
  ...group(Js, 'js', 'mjs', 'cjs'),
  jsx: Reactjs,

  // Systems languages
  go: Go, rs: Rust, java: Java,
  ...group(Kotlin, 'kt', 'kts'),
  swift: Swift,
  ...group(CLang, 'c', 'h'),
  ...group(Cplus, 'cpp', 'cc', 'cxx', 'hpp', 'cu'),
  ...group(Csharp, 'cs', 'csx'),
  ...group(Ruby, 'rb', 'erb'),
  php: PHP,
  lua: Lua, luau: Luau,
  ...group(Shell, 'sh', 'bash', 'zsh', 'fish', 'ksh', 'csh', 'bat', 'cmd', 'ps1', 'psm1', 'awk', 'nu'),
  dart: Dart,
  ...group(Scala, 'scala', 'sc', 'sbt'),
  ...group(Clojure, 'clj', 'cljs', 'cljc', 'edn'),
  hs: Haskell, erl: Erlang,
  ...group(Elixir, 'ex', 'exs', 'eex', 'leex', 'heex'),
  nim: Nim, v: Vlang, zig: Zig, sol: Solidity,
  ...group(R, 'r', 'rmd'),
  jl: Julia,
  ...group(Perl, 'pl', 'pm'),
  ...group(Fortran, 'f', 'f90', 'f95', 'f03', 'for'),
  ...group(Fsharp, 'fs', 'fsx', 'fsi'),
  res: Rescript, resi: RescriptInterface,
  gleam: Gleam, coffee: CoffeeScript,

  // Query / schema
  ...group(Graphql, 'graphql', 'gql'),
  ...group(Database, 'sql', 'pks', 'pkb', 'sqlite', 'db'),
  proto: Proto,

  // Web markup — no dedicated HTML/CSS icons in Symbols, use Code variants
  ...group(CodeOrange, 'html', 'htm', 'shtml'),
  css: CodeBlue, less: CodeBlue, styl: Stylus,
  ...group(Sass, 'scss', 'sass'),
  vue: Vue, svelte: Svelte, astro: Astro,

  // Template languages
  haml: Haml,
  ...group(Pug, 'pug', 'jade'),
  twig: Twig, liquid: Liquid,
  ...group(Nunjucks, 'njk', 'nunjucks'),
  ...group(Coldfusion, 'cfml', 'cfc', 'cfm'),

  // Data / config
  ...group(BracketsYellow, 'json', 'jsonc', 'json5'),
  ...group(Yaml, 'yaml', 'yml'),
  ...group(Gear, 'toml', 'ini', 'cfg', 'conf'),
  ...group(XML, 'xml', 'plist', 'xsd', 'xsl', 'xslt'),
  ...group(Lock, 'env', 'lock'),

  // Spreadsheets / tables
  ...group(Csv, 'csv', 'tsv', 'psv', 'xlsx', 'xls', 'ods'),

  // Markdown / docs
  ...group(Markdown, 'md', 'mdoc'),
  mdx: MDX,
  ...group(Text, 'txt', 'tex', 'log'),
  ...group(Document, 'rtf', 'doc', 'docx', 'epub'),
  pdf: PDF, license: License,

  // Media
  ...group(Image, 'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'avif', 'tiff', 'tif', 'heic', 'heif', 'psd', 'raw', 'eps'),
  svg: SVG,
  ...group(Video, 'mp4', 'webm', 'mov', 'mkv', 'avi', 'ogv', 'm4v', 'wmv', 'flv'),
  ...group(Audio, 'mp3', 'wav', 'ogg', 'oga', 'flac', 'm4a', 'aac', 'opus', 'wma', 'aiff'),

  // 3D models — Puzzle icon distinguishes from archives
  ...group(Puzzle, 'stl', 'step', 'stp', 'obj', '3mf', 'gltf', 'glb', 'ply', 'dae', 'wrl', 'vrml'),

  // Archives — Zip icon clearly identifies them
  ...group(Zip, 'zip', 'tar', 'gz', 'bz2', '7z', 'rar', 'xz', 'iso', 'deb', 'rpm', 'dmg'),

  // Binaries
  ...group(Exe, 'exe', 'msi', 'bin'),
  ...group(Font, 'woff', 'woff2', 'ttf', 'otf', 'eot'),
  ipynb: Notebook,
  ...group(Patch, 'patch', 'diff'),
  ...group(I18n, 'po', 'pot', 'mo', 'lang'),
  ...group(Http, 'http', 'rest'),
}

// ---------------------------------------------------------------------------
// Exact filename → icon map (checked before extension lookup)
// ---------------------------------------------------------------------------

const NAME_MAP: Record<string, FileIconComponent> = {
  // Package managers
  ...group(NPM, 'package.json', 'package-lock.json', '.npmrc', '.npmignore'),
  ...group(PNPM, 'pnpm-lock.yaml', 'pnpm-workspace.yaml', '.pnpmfile.cjs'),
  ...group(Yarn, 'yarn.lock'),

  // TypeScript config
  ...group(Tsconfig, 'tsconfig.json', 'tsconfig.app.json', 'tsconfig.base.json', 'tsconfig.build.json', 'tsconfig.node.json', 'tsconfig.eslint.json', 'tsconfig.spec.json'),
  'tsconfig.test.json': TsTest,

  // Bundlers / build tools
  ...group(Vite, 'vite.config.ts', 'vite.config.js', 'vite.config.mjs', 'vite.config.cjs', 'vite.base.config.ts'),
  ...group(Webpack, 'webpack.config.js', 'webpack.config.ts', 'webpack.config.cjs', 'webpack.config.mjs'),
  'turbo.json': Turborepo,

  // Go / Rust
  ...group(GoMod, 'go.mod', 'go.sum', 'go.work', 'go.work.sum'),
  ...group(Rust, 'cargo.toml', 'cargo.lock'),

  // Python
  ...group(Python, 'requirements.txt', 'pipfile', 'pipfile.lock', 'pyproject.toml', 'setup.py', 'setup.cfg', '.python-version', 'manifest.in', 'pylintrc', '.pylintrc'),

  // Docker
  ...group(Docker, 'dockerfile', 'dockerfile.dev', 'dockerfile.prod', 'dockerfile.production', 'dockerfile.staging', 'dockerfile.test', 'docker-compose.yml', 'docker-compose.yaml', 'compose.yml', 'compose.yaml', '.dockerignore', 'docker-healthcheck'),

  // Git
  ...group(Ignore, '.gitignore', '.gitattributes', '.gitmodules', '.gitkeep', '.git-blame-ignore'),
  '.gitconfig': Git,

  // CI / deployment
  ...group(Vercel, '.vercel', 'vercel.json', 'vercel.toml'),
  ...group(Netlify, 'netlify.json', 'netlify.toml', 'netlify.yml'),

  // Linters / formatters
  ...group(Eslint, '.eslintrc', '.eslintrc.js', '.eslintrc.cjs', '.eslintrc.json', '.eslintrc.yaml', '.eslintrc.yml', '.eslintignore', 'eslint.config.js', 'eslint.config.ts', 'eslint.config.mjs', 'eslint.config.cjs'),
  ...group(Prettier, '.prettierrc', '.prettierrc.js', '.prettierrc.json', '.prettierrc.yaml', '.prettierrc.yml', 'prettier.config.js', 'prettier.config.ts', 'prettier.config.mjs', '.prettierignore'),
  ...group(Stylelint, '.stylelintrc', '.stylelintrc.js', '.stylelintrc.json', 'stylelint.config.js', 'stylelint.config.cjs', '.stylelintignore'),
  ...group(Biome, 'biome.json', 'biome.jsonc'),

  // Testing
  ...group(Jest, 'jest.config.js', 'jest.config.ts', 'jest.config.cjs', 'jest.config.mjs', 'jest.config.json', 'jest.setup.js', 'jest.setup.ts'),
  ...group(Vitest, 'vitest.config.ts', 'vitest.config.js', 'vitest.config.mjs', 'vitest.config.cjs'),

  // Frameworks
  ...group(Next, 'next.config.js', 'next.config.mjs', 'next.config.ts'),
  ...group(Nuxt, 'nuxt.config.js', 'nuxt.config.ts', 'nuxt.config.mjs'),
  ...group(Svelte, 'svelte.config.js', 'svelte.config.ts'),
  ...group(Astro, 'astro.config.mjs', 'astro.config.ts', 'astro.config.cjs'),
  ...group(Tailwind, 'tailwind.config.js', 'tailwind.config.ts', 'tailwind.config.cjs', 'tailwind.config.mjs'),
  ...group(PostCSS, 'postcss.config.js', 'postcss.config.ts', 'postcss.config.cjs', 'postcss.config.mjs', '.postcssrc', '.postcssrc.js', '.postcssrc.json'),

  // DB / ORM
  ...group(Prisma, 'prisma.yml', 'schema.prisma'),

  // Storybook
  '.storybook': Storybook,

  // Env files
  ...group(Lock, '.env', '.env.local', '.env.development', '.env.production', '.env.test', '.env.example', '.env.dev', '.env.prod'),

  // Build systems
  'makefile': Gear, 'jenkinsfile': Gear,
  ...group(CMake, 'cmakelists.txt', 'cmakecache.txt'),
  ...group(Gradle, 'gradlew', 'gradle.properties', 'gradle-wrapper.properties'),

  // Docs
  ...group(Markdown, 'readme', 'readme.md', 'changelog', 'changelog.md', 'contributing', 'contributing.md'),
  'readme.txt': Text,
  ...group(License, 'license', 'license.md', 'license.txt'),

  // Misc
  'components.json': Shadcn,
  '.editorconfig': EditorConfig,
  ...group(Cursor, '.cursorrules', '.cursorignore'),
  ...group(Claude, 'claude.md', '.claude', '.clauderc', '.claudeignore'),
  ...group(Knip, 'knip.json', 'knip.ts', 'knip.config.ts'),
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/**
 * Resolves the icon component for a file based on its name/extension.
 * Checks exact filename matches first (e.g. "Dockerfile", "package.json"),
 * then falls back to extension lookup. Returns a default file icon if no
 * match is found.
 */
export function fileIcon(name: string): FileIconComponent {
  const lower = name.toLowerCase()
  if (NAME_MAP[lower]) return NAME_MAP[lower]
  const dot = lower.lastIndexOf('.')
  if (dot >= 0) {
    const ext = lower.slice(dot + 1)
    if (EXT_MAP[ext]) return EXT_MAP[ext]
  }
  return DefaultFileIcon
}

/** Renders the appropriate file-type icon for the given filename. */
export function FileIcon({ name, ...props }: { name: string } & SVGProps<SVGSVGElement>) {
  return createElement(fileIcon(name), props)
}
