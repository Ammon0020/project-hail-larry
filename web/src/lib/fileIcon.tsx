/**
 * fileIcon — maps file names/extensions to VS Code Symbols-style SVG icons.
 *
 * Uses @react-symbols/icons (Miguel Solorio's Symbols icon theme) which
 * provides language-specific colored SVG glyphs — the same style VS Code
 * uses. Icons are imported individually from the package's named exports so
 * Vite tree-shakes unused icons out of the bundle.
 *
 * Used by the file tree (FileTree.tsx) and tab bar (TabBar.tsx).
 */
import {
  TypeScript, Tsconfig, TsTest,
  Js,
  Reactts, Reactjs,
  Go, GoMod,
  Python,
  Rust,
  Java,
  Kotlin,
  Swift,
  CLang, Cplus, Csharp,
  Ruby,
  PHP,
  Lua, Luau,
  Shell,
  Dart,
  Scala,
  Clojure,
  Haskell,
  Erlang,
  Elixir,
  Nim,
  Vlang,
  Zig,
  Solidity,
  R,
  Julia,
  Perl,
  Fortran,
  Fsharp,
  Rescript,
  Gleam,
  CoffeeScript,
  Graphql,
  Proto,
  Sass,
  Vue,
  Svelte,
  Astro,
  Next,
  Nuxt,
  Tailwind,
  PostCSS,
  Markdown, MDX,
  Yaml,
  XML,
  Csv,
  Image,
  SVG,
  Video,
  Audio,
  PDF,
  Document,
  Text,
  Lock,
  Ignore,
  License,
  Git,
  Docker,
  NPM,
  PNPM,
  Yarn,
  Vite,
  Vitest,
  Jest,
  Eslint,
  Prettier,
  Storybook,
  Prisma,
  Database,
  Gear,
  Zip,
  Puzzle,
  Exe,
  Font,
  Notebook,
  Vercel,
  Netlify,
  Stylelint,
  Biome,
  Webpack,
  Turborepo,
  CMake,
  Gradle,
  Shadcn,
  CodeOrange,
  CodeBlue,
  BracketsYellow,
  Http,
  Haml,
  Pug,
  Twig,
  Liquid,
  Coldfusion,
  Nunjucks,
  Stylus,
  RescriptInterface,
  Dts,
  Patch,
  I18n,
  EditorConfig,
  Knip,
  Cursor,
  Claude,
} from '@react-symbols/icons/files'

import { DefaultFileIcon } from '@react-symbols/icons/utils'
import type { ComponentType, SVGProps } from 'react'

/** A file-type icon component from @react-symbols/icons. */
export type FileIconComponent = ComponentType<SVGProps<SVGSVGElement>>

/** A file-type icon + optional Tailwind color override. Most Symbols icons
 *  are already colored, so color is rarely needed — but it's here for cases
 *  where we want to force a color (e.g. in the tab bar where space is tight). */
export interface FileIconInfo {
  icon: FileIconComponent
}

/** Maps file extensions to their icon component. */
const EXT_MAP: Record<string, FileIconInfo> = {
  // TypeScript / JavaScript
  ts:    { icon: TypeScript },
  mts:   { icon: TypeScript },
  cts:   { icon: TypeScript },
  tsx:   { icon: Reactts },
  js:    { icon: Js },
  mjs:   { icon: Js },
  cjs:   { icon: Js },
  jsx:   { icon: Reactjs },

  // Go
  go:    { icon: Go },

  // Python
  py:    { icon: Python },
  pyc:   { icon: Python },
  pyw:   { icon: Python },

  // Rust
  rs:    { icon: Rust },

  // Java / Kotlin
  java:  { icon: Java },
  kt:    { icon: Kotlin },
  kts:   { icon: Kotlin },

  // Swift
  swift: { icon: Swift },

  // C / C++ / C#
  c:     { icon: CLang },
  h:     { icon: CLang },
  cpp:   { icon: Cplus },
  cc:    { icon: Cplus },
  cxx:   { icon: Cplus },
  hpp:   { icon: Cplus },
  cu:    { icon: Cplus },
  cs:    { icon: Csharp },
  csx:   { icon: Csharp },

  // Ruby
  rb:    { icon: Ruby },
  erb:   { icon: Ruby },

  // PHP
  php:   { icon: PHP },

  // Lua
  lua:   { icon: Lua },
  luau:  { icon: Luau },

  // Shell
  sh:    { icon: Shell },
  bash:  { icon: Shell },
  zsh:   { icon: Shell },
  fish:  { icon: Shell },
  ksh:   { icon: Shell },
  csh:   { icon: Shell },
  bat:   { icon: Shell },
  cmd:   { icon: Shell },
  ps1:   { icon: Shell },
  psm1:  { icon: Shell },
  awk:   { icon: Shell },
  nu:    { icon: Shell },

  // Dart
  dart:  { icon: Dart },

  // Scala
  scala: { icon: Scala },
  sc:    { icon: Scala },
  sbt:   { icon: Scala },

  // Clojure
  clj:   { icon: Clojure },
  cljs:  { icon: Clojure },
  cljc:  { icon: Clojure },
  edn:   { icon: Clojure },

  // Haskell
  hs:    { icon: Haskell },

  // Erlang / Elixir
  erl:   { icon: Erlang },
  ex:    { icon: Elixir },
  exs:   { icon: Elixir },
  eex:   { icon: Elixir },
  leex:  { icon: Elixir },
  heex:  { icon: Elixir },

  // Nim
  nim:   { icon: Nim },

  // V
  v:     { icon: Vlang },

  // Zig
  zig:   { icon: Zig },

  // Solidity
  sol:   { icon: Solidity },

  // R
  r:     { icon: R },
  rmd:   { icon: R },

  // Julia
  jl:    { icon: Julia },

  // Perl
  pl:    { icon: Perl },
  pm:    { icon: Perl },

  // Fortran
  f:     { icon: Fortran },
  f90:   { icon: Fortran },
  f95:   { icon: Fortran },
  f03:   { icon: Fortran },
  for:   { icon: Fortran },

  // F#
  fs:    { icon: Fsharp },
  fsx:   { icon: Fsharp },
  fsi:   { icon: Fsharp },

  // ReScript
  res:   { icon: Rescript },
  resi:  { icon: RescriptInterface },

  // Gleam
  gleam: { icon: Gleam },

  // CoffeeScript
  coffee:{ icon: CoffeeScript },

  // GraphQL
  graphql:{ icon: Graphql },
  gql:   { icon: Graphql },

  // SQL
  sql:   { icon: Database },
  pks:   { icon: Database },
  pkb:   { icon: Database },
  sqlite:{ icon: Database },
  db:    { icon: Database },

  // Protobuf
  proto: { icon: Proto },

  // Web markup — no dedicated HTML/CSS icons in Symbols, use Code variants
  html:  { icon: CodeOrange },
  htm:   { icon: CodeOrange },
  shtml: { icon: CodeOrange },
  css:   { icon: CodeBlue },
  scss:  { icon: Sass },
  sass:  { icon: Sass },
  less:  { icon: CodeBlue },
  styl:  { icon: Stylus },
  vue:   { icon: Vue },
  svelte:{ icon: Svelte },
  astro: { icon: Astro },

  // Template languages
  haml:  { icon: Haml },
  pug:   { icon: Pug },
  jade:  { icon: Pug },
  twig:  { icon: Twig },
  liquid:{ icon: Liquid },
  njk:   { icon: Nunjucks },
  nunjucks:{ icon: Nunjucks },
  cfml:  { icon: Coldfusion },
  cfc:   { icon: Coldfusion },
  cfm:   { icon: Coldfusion },

  // Data / config — no dedicated JSON/TOML icons, use substitutes
  json:  { icon: BracketsYellow },
  jsonc: { icon: BracketsYellow },
  json5: { icon: BracketsYellow },
  yaml:  { icon: Yaml },
  yml:   { icon: Yaml },
  toml:  { icon: Gear },
  xml:   { icon: XML },
  plist: { icon: XML },
  xsd:   { icon: XML },
  xsl:   { icon: XML },
  xslt:  { icon: XML },
  csv:   { icon: Csv },
  tsv:   { icon: Csv },
  psv:   { icon: Csv },
  ini:   { icon: Gear },
  cfg:   { icon: Gear },
  conf:  { icon: Gear },
  env:   { icon: Lock },
  lock:  { icon: Lock },

  // Markdown / docs
  md:    { icon: Markdown },
  mdx:   { icon: MDX },
  mdoc:  { icon: Markdown },
  txt:   { icon: Text },
  rtf:   { icon: Document },
  pdf:   { icon: PDF },
  doc:   { icon: Document },
  docx:  { icon: Document },
  epub:  { icon: Document },
  tex:   { icon: Text },
  license:{ icon: License },

  // Spreadsheets
  xlsx:  { icon: Csv },
  xls:   { icon: Csv },
  ods:   { icon: Csv },

  // Images
  png:   { icon: Image },
  jpg:   { icon: Image },
  jpeg:  { icon: Image },
  gif:   { icon: Image },
  webp:  { icon: Image },
  bmp:   { icon: Image },
  ico:   { icon: Image },
  avif:  { icon: Image },
  tiff:  { icon: Image },
  tif:   { icon: Image },
  heic:  { icon: Image },
  heif:  { icon: Image },
  svg:   { icon: SVG },
  psd:   { icon: Image },
  raw:   { icon: Image },
  eps:   { icon: Image },

  // Video
  mp4:   { icon: Video },
  webm:  { icon: Video },
  mov:   { icon: Video },
  mkv:   { icon: Video },
  avi:   { icon: Video },
  ogv:   { icon: Video },
  m4v:   { icon: Video },
  wmv:   { icon: Video },
  flv:   { icon: Video },

  // Audio
  mp3:   { icon: Audio },
  wav:   { icon: Audio },
  ogg:   { icon: Audio },
  oga:   { icon: Audio },
  flac:  { icon: Audio },
  m4a:   { icon: Audio },
  aac:   { icon: Audio },
  opus:  { icon: Audio },
  wma:   { icon: Audio },
  aiff:  { icon: Audio },

  // 3D models — use Puzzle to distinguish from archives
  stl:   { icon: Puzzle },
  step:  { icon: Puzzle },
  stp:   { icon: Puzzle },
  obj:   { icon: Puzzle },
  '3mf': { icon: Puzzle },
  gltf:  { icon: Puzzle },
  glb:   { icon: Puzzle },
  ply:   { icon: Puzzle },
  dae:   { icon: Puzzle },
  wrl:   { icon: Puzzle },
  vrml:  { icon: Puzzle },

  // Archives — use Zip icon so they're clearly archive files
  zip:   { icon: Zip },
  tar:   { icon: Zip },
  gz:    { icon: Zip },
  bz2:   { icon: Zip },
  '7z':  { icon: Zip },
  rar:   { icon: Zip },
  xz:    { icon: Zip },
  iso:   { icon: Zip },
  deb:   { icon: Zip },
  rpm:   { icon: Zip },
  dmg:   { icon: Zip },

  // Executables / binaries
  exe:   { icon: Exe },
  msi:   { icon: Exe },
  bin:   { icon: Exe },

  // Fonts
  woff:  { icon: Font },
  woff2: { icon: Font },
  ttf:   { icon: Font },
  otf:   { icon: Font },
  eot:   { icon: Font },

  // Notebook
  ipynb: { icon: Notebook },

  // TypeScript declaration files
  'd.ts': { icon: Dts },
  'd.cts':{ icon: Dts },
  'd.mts':{ icon: Dts },

  // Log
  log:   { icon: Text },

  // Patch / diff
  patch: { icon: Patch },
  diff:  { icon: Patch },

  // i18n
  po:    { icon: I18n },
  pot:   { icon: I18n },
  mo:    { icon: I18n },
  lang:  { icon: I18n },

  // HTTP / REST
  http:  { icon: Http },
  rest:  { icon: Http },
}

/** Exact filename matches for config/special files. */
const NAME_MAP: Record<string, FileIconInfo> = {
  // Build / package managers
  'package.json':       { icon: NPM },
  'package-lock.json':  { icon: NPM },
  'pnpm-lock.yaml':     { icon: PNPM },
  'pnpm-workspace.yaml':{ icon: PNPM },
  '.pnpmfile.cjs':      { icon: PNPM },
  'yarn.lock':          { icon: Yarn },
  '.npmrc':             { icon: NPM },
  '.npmignore':         { icon: NPM },

  // TypeScript config
  'tsconfig.json':      { icon: Tsconfig },
  'tsconfig.app.json':  { icon: Tsconfig },
  'tsconfig.base.json': { icon: Tsconfig },
  'tsconfig.build.json':{ icon: Tsconfig },
  'tsconfig.node.json': { icon: Tsconfig },
  'tsconfig.eslint.json':{ icon: Tsconfig },
  'tsconfig.spec.json': { icon: Tsconfig },
  'tsconfig.test.json': { icon: TsTest },

  // Vite / bundlers
  'vite.config.ts':     { icon: Vite },
  'vite.config.js':     { icon: Vite },
  'vite.config.mjs':    { icon: Vite },
  'vite.config.cjs':    { icon: Vite },
  'vite.base.config.ts':{ icon: Vite },
  'webpack.config.js':  { icon: Webpack },
  'webpack.config.ts':  { icon: Webpack },
  'webpack.config.cjs': { icon: Webpack },
  'webpack.config.mjs': { icon: Webpack },
  'turbo.json':         { icon: Turborepo },

  // Go
  'go.mod':             { icon: GoMod },
  'go.sum':             { icon: GoMod },
  'go.work':            { icon: GoMod },
  'go.work.sum':        { icon: GoMod },

  // Rust — no Cargo icon in Symbols, use Rust
  'cargo.toml':         { icon: Rust },
  'cargo.lock':         { icon: Rust },

  // Python
  'requirements.txt':   { icon: Python },
  'pipfile':            { icon: Python },
  'pipfile.lock':       { icon: Python },
  'pyproject.toml':     { icon: Python },
  'setup.py':           { icon: Python },
  'setup.cfg':          { icon: Python },
  '.python-version':    { icon: Python },
  'manifest.in':        { icon: Python },
  'pylintrc':           { icon: Python },
  '.pylintrc':          { icon: Python },

  // Docker — no Dockerfile icon, use Docker
  'dockerfile':         { icon: Docker },
  'dockerfile.dev':     { icon: Docker },
  'dockerfile.prod':    { icon: Docker },
  'dockerfile.production': { icon: Docker },
  'dockerfile.staging': { icon: Docker },
  'dockerfile.test':    { icon: Docker },
  'docker-compose.yml': { icon: Docker },
  'docker-compose.yaml':{ icon: Docker },
  'compose.yml':        { icon: Docker },
  'compose.yaml':       { icon: Docker },
  '.dockerignore':      { icon: Docker },

  // Git
  '.gitignore':         { icon: Ignore },
  '.gitattributes':     { icon: Ignore },
  '.gitmodules':        { icon: Ignore },
  '.gitkeep':           { icon: Ignore },
  '.gitconfig':         { icon: Git },
  '.git-blame-ignore':  { icon: Ignore },

  // CI / deployment
  '.vercel':            { icon: Vercel },
  'vercel.json':        { icon: Vercel },
  'vercel.toml':        { icon: Vercel },
  'netlify.json':       { icon: Netlify },
  'netlify.toml':       { icon: Netlify },
  'netlify.yml':        { icon: Netlify },

  // Linters / formatters
  '.eslintrc':          { icon: Eslint },
  '.eslintrc.js':       { icon: Eslint },
  '.eslintrc.cjs':      { icon: Eslint },
  '.eslintrc.json':     { icon: Eslint },
  '.eslintrc.yaml':     { icon: Eslint },
  '.eslintrc.yml':      { icon: Eslint },
  'eslint.config.js':   { icon: Eslint },
  'eslint.config.ts':   { icon: Eslint },
  'eslint.config.mjs':  { icon: Eslint },
  'eslint.config.cjs':  { icon: Eslint },
  '.eslintignore':      { icon: Eslint },
  '.prettierrc':        { icon: Prettier },
  '.prettierrc.js':     { icon: Prettier },
  '.prettierrc.json':   { icon: Prettier },
  '.prettierrc.yaml':   { icon: Prettier },
  '.prettierrc.yml':    { icon: Prettier },
  'prettier.config.js': { icon: Prettier },
  'prettier.config.ts': { icon: Prettier },
  'prettier.config.mjs':{ icon: Prettier },
  '.prettierignore':    { icon: Prettier },
  'biome.json':         { icon: Biome },
  'biome.jsonc':        { icon: Biome },
  '.stylelintrc':       { icon: Stylelint },
  'stylelint.config.js':{ icon: Stylelint },
  'stylelint.config.cjs':{ icon: Stylelint },
  '.stylelintrc.json':  { icon: Stylelint },
  '.stylelintrc.js':    { icon: Stylelint },
  '.stylelintignore':   { icon: Stylelint },

  // Testing
  'jest.config.js':     { icon: Jest },
  'jest.config.ts':     { icon: Jest },
  'jest.config.cjs':    { icon: Jest },
  'jest.config.mjs':    { icon: Jest },
  'jest.config.json':   { icon: Jest },
  'jest.setup.js':      { icon: Jest },
  'jest.setup.ts':      { icon: Jest },
  'vitest.config.ts':   { icon: Vitest },
  'vitest.config.js':   { icon: Vitest },
  'vitest.config.mjs':  { icon: Vitest },
  'vitest.config.cjs':  { icon: Vitest },

  // Frameworks
  'next.config.js':     { icon: Next },
  'next.config.mjs':    { icon: Next },
  'next.config.ts':     { icon: Next },
  'nuxt.config.js':     { icon: Nuxt },
  'nuxt.config.ts':     { icon: Nuxt },
  'nuxt.config.mjs':    { icon: Nuxt },
  'svelte.config.js':   { icon: Svelte },
  'svelte.config.ts':   { icon: Svelte },
  'astro.config.mjs':   { icon: Astro },
  'astro.config.ts':    { icon: Astro },
  'astro.config.cjs':   { icon: Astro },
  'tailwind.config.js': { icon: Tailwind },
  'tailwind.config.ts': { icon: Tailwind },
  'tailwind.config.cjs':{ icon: Tailwind },
  'tailwind.config.mjs':{ icon: Tailwind },
  'postcss.config.js':  { icon: PostCSS },
  'postcss.config.ts':  { icon: PostCSS },
  'postcss.config.cjs': { icon: PostCSS },
  'postcss.config.mjs': { icon: PostCSS },
  '.postcssrc':         { icon: PostCSS },
  '.postcssrc.js':      { icon: PostCSS },
  '.postcssrc.json':    { icon: PostCSS },

  // Prisma / DB
  'prisma.yml':         { icon: Prisma },
  'schema.prisma':      { icon: Prisma },

  // Storybook
  '.storybook':         { icon: Storybook },

  // Env
  '.env':               { icon: Lock },
  '.env.local':         { icon: Lock },
  '.env.development':   { icon: Lock },
  '.env.production':    { icon: Lock },
  '.env.test':          { icon: Lock },
  '.env.example':       { icon: Lock },
  '.env.dev':           { icon: Lock },
  '.env.prod':          { icon: Lock },

  // Build systems — no Makefile icon, use Gear
  'makefile':           { icon: Gear },
  'cmakelists.txt':     { icon: CMake },
  'cmakecache.txt':     { icon: CMake },
  'gradlew':            { icon: Gradle },
  'gradle.properties':  { icon: Gradle },
  'gradle-wrapper.properties': { icon: Gradle },
  'jenkinsfile':        { icon: Gear },

  // Docs
  'readme':             { icon: Markdown },
  'readme.md':          { icon: Markdown },
  'readme.txt':         { icon: Text },
  'license':            { icon: License },
  'license.md':         { icon: License },
  'license.txt':        { icon: License },
  'changelog':          { icon: Markdown },
  'changelog.md':       { icon: Markdown },
  'contributing':       { icon: Markdown },
  'contributing.md':    { icon: Markdown },

  // Misc
  'components.json':    { icon: Shadcn },
  'docker-healthcheck': { icon: Docker },
  '.editorconfig':      { icon: EditorConfig },
  '.cursorrules':       { icon: Cursor },
  '.cursorignore':      { icon: Cursor },
  'claude.md':          { icon: Claude },
  '.claude':            { icon: Claude },
  '.clauderc':          { icon: Claude },
  '.claudeignore':      { icon: Claude },
  'knip.json':          { icon: Knip },
  'knip.ts':            { icon: Knip },
  'knip.config.ts':     { icon: Knip },
}

/** Default icon for unrecognized file extensions. */
const DEFAULT_ICON: FileIconInfo = {
  icon: DefaultFileIcon,
}

/**
 * Resolves the icon for a file based on its name/extension. Checks exact
 * filename matches first (e.g. "Dockerfile", "package.json"), then falls
 * back to extension lookup. Returns a default file icon if no match is found.
 */
export function fileIcon(name: string): FileIconInfo {
  const lower = name.toLowerCase()
  if (NAME_MAP[lower]) return NAME_MAP[lower]
  const dot = lower.lastIndexOf('.')
  if (dot >= 0) {
    const ext = lower.slice(dot + 1)
    if (EXT_MAP[ext]) return EXT_MAP[ext]
  }
  return DEFAULT_ICON
}
