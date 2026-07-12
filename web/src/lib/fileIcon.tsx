/**
 * fileIcon — maps file extensions to Lucide icons with semantic colors.
 *
 * Used by the file tree (FileTree.tsx) and tab bar (TabBar.tsx) to show
 * type-specific icons instead of a generic file glyph for every file.
 * The mapping is extension-based and computed in the frontend — the backend
 * does not need to send icon metadata.
 */
import {
  FileCode, FileText, FileImage, FileVideo, FileAudio, FileJson,
  FileArchive, FileSpreadsheet, FileBox, FileLock, FileBraces,
  FileCog, FileTerminal, FileType,
  type LucideIcon,
} from 'lucide-react'

/** A file-type icon + Tailwind color class pair. */
export interface FileIconInfo {
  icon: LucideIcon
  color: string
}

/** Maps a file name to its icon and color, based on the extension. */
const EXT_MAP: Record<string, FileIconInfo> = {
  // Code — yellow (VS Code convention)
  ts:   { icon: FileCode, color: 'text-blue-400' },
  tsx:  { icon: FileCode, color: 'text-blue-400' },
  js:   { icon: FileBraces, color: 'text-yellow-400' },
  jsx:  { icon: FileBraces, color: 'text-yellow-400' },
  mjs:  { icon: FileBraces, color: 'text-yellow-400' },
  cjs:  { icon: FileBraces, color: 'text-yellow-400' },
  go:   { icon: FileCode, color: 'text-cyan-400' },
  py:   { icon: FileCode, color: 'text-green-400' },
  rs:   { icon: FileCode, color: 'text-orange-400' },
  rb:   { icon: FileCode, color: 'text-red-400' },
  java: { icon: FileCode, color: 'text-orange-400' },
  kt:   { icon: FileCode, color: 'text-purple-400' },
  swift:{ icon: FileCode, color: 'text-orange-400' },
  c:    { icon: FileCode, color: 'text-blue-400' },
  cpp:  { icon: FileCode, color: 'text-blue-400' },
  h:    { icon: FileCode, color: 'text-blue-400' },
  hpp:  { icon: FileCode, color: 'text-blue-400' },
  cs:   { icon: FileCode, color: 'text-green-400' },
  php:  { icon: FileCode, color: 'text-indigo-400' },
  lua:  { icon: FileCode, color: 'text-blue-400' },
  sh:   { icon: FileTerminal, color: 'text-green-400' },
  bash: { icon: FileTerminal, color: 'text-green-400' },
  zsh:  { icon: FileTerminal, color: 'text-green-400' },
  fish: { icon: FileTerminal, color: 'text-green-400' },
  ps1:  { icon: FileTerminal, color: 'text-blue-400' },
  bat:  { icon: FileTerminal, color: 'text-green-400' },

  // Web markup
  html: { icon: FileCode, color: 'text-orange-400' },
  htm:  { icon: FileCode, color: 'text-orange-400' },
  css:  { icon: FileCode, color: 'text-blue-400' },
  scss: { icon: FileCode, color: 'text-pink-400' },
  sass: { icon: FileCode, color: 'text-pink-400' },
  less: { icon: FileCode, color: 'text-blue-400' },
  vue:  { icon: FileCode, color: 'text-green-400' },
  svelte: { icon: FileCode, color: 'text-orange-400' },
  astro:{ icon: FileCode, color: 'text-purple-400' },

  // Data / config
  json: { icon: FileJson, color: 'text-yellow-400' },
  yaml: { icon: FileJson, color: 'text-yellow-400' },
  yml:  { icon: FileJson, color: 'text-yellow-400' },
  toml: { icon: FileJson, color: 'text-yellow-400' },
  xml:  { icon: FileCode, color: 'text-orange-400' },
  ini:  { icon: FileCog, color: 'text-muted-foreground' },
  env:  { icon: FileLock, color: 'text-yellow-400' },
  csv:  { icon: FileSpreadsheet, color: 'text-green-400' },
  tsv:  { icon: FileSpreadsheet, color: 'text-green-400' },

  // Markdown / docs
  md:   { icon: FileText, color: 'text-blue-400' },
  mdx:  { icon: FileText, color: 'text-blue-400' },
  txt:  { icon: FileText, color: 'text-muted-foreground' },
  rtf:  { icon: FileText, color: 'text-muted-foreground' },
  pdf:  { icon: FileText, color: 'text-red-400' },
  doc:  { icon: FileText, color: 'text-blue-400' },
  docx: { icon: FileText, color: 'text-blue-400' },
  epub: { icon: FileText, color: 'text-orange-400' },

  // Spreadsheets
  xlsx: { icon: FileSpreadsheet, color: 'text-green-400' },
  xls:  { icon: FileSpreadsheet, color: 'text-green-400' },
  ods:  { icon: FileSpreadsheet, color: 'text-green-400' },

  // Images
  png:  { icon: FileImage, color: 'text-purple-400' },
  jpg:  { icon: FileImage, color: 'text-purple-400' },
  jpeg: { icon: FileImage, color: 'text-purple-400' },
  gif:  { icon: FileImage, color: 'text-purple-400' },
  webp: { icon: FileImage, color: 'text-purple-400' },
  bmp:  { icon: FileImage, color: 'text-purple-400' },
  ico:  { icon: FileImage, color: 'text-purple-400' },
  avif: { icon: FileImage, color: 'text-purple-400' },
  tiff: { icon: FileImage, color: 'text-purple-400' },
  tif:  { icon: FileImage, color: 'text-purple-400' },
  heic: { icon: FileImage, color: 'text-purple-400' },
  heif: { icon: FileImage, color: 'text-purple-400' },
  svg:  { icon: FileImage, color: 'text-yellow-400' },

  // Video
  mp4:  { icon: FileVideo, color: 'text-red-400' },
  webm: { icon: FileVideo, color: 'text-red-400' },
  mov:  { icon: FileVideo, color: 'text-red-400' },
  mkv:  { icon: FileVideo, color: 'text-red-400' },
  avi:  { icon: FileVideo, color: 'text-red-400' },
  ogv:  { icon: FileVideo, color: 'text-red-400' },

  // Audio
  mp3:  { icon: FileAudio, color: 'text-pink-400' },
  wav:  { icon: FileAudio, color: 'text-pink-400' },
  ogg:  { icon: FileAudio, color: 'text-pink-400' },
  oga:  { icon: FileAudio, color: 'text-pink-400' },
  flac: { icon: FileAudio, color: 'text-pink-400' },
  m4a:  { icon: FileAudio, color: 'text-pink-400' },
  aac:  { icon: FileAudio, color: 'text-pink-400' },
  opus: { icon: FileAudio, color: 'text-pink-400' },

  // 3D models
  stl:  { icon: FileBox, color: 'text-orange-400' },
  obj:  { icon: FileBox, color: 'text-orange-400' },
  '3mf':{ icon: FileBox, color: 'text-orange-400' },
  gltf: { icon: FileBox, color: 'text-orange-400' },
  glb:  { icon: FileBox, color: 'text-orange-400' },
  ply:  { icon: FileBox, color: 'text-orange-400' },
  dae:  { icon: FileBox, color: 'text-orange-400' },
  wrl:  { icon: FileBox, color: 'text-orange-400' },
  vrml: { icon: FileBox, color: 'text-orange-400' },

  // Archives
  zip:  { icon: FileArchive, color: 'text-yellow-400' },
  tar:  { icon: FileArchive, color: 'text-yellow-400' },
  gz:   { icon: FileArchive, color: 'text-yellow-400' },
  bz2:  { icon: FileArchive, color: 'text-yellow-400' },
  '7z': { icon: FileArchive, color: 'text-yellow-400' },
  rar:  { icon: FileArchive, color: 'text-yellow-400' },
  xz:   { icon: FileArchive, color: 'text-yellow-400' },

  // Lockfiles / special
  lock: { icon: FileLock, color: 'text-muted-foreground' },
  log:  { icon: FileText, color: 'text-muted-foreground' },
  sql:  { icon: FileCode, color: 'text-cyan-400' },
  graphql: { icon: FileCode, color: 'text-pink-400' },
  gql:  { icon: FileCode, color: 'text-pink-400' },
  proto:{ icon: FileCode, color: 'text-blue-400' },
  dockerfile: { icon: FileCog, color: 'text-blue-400' },

  // Config files (by exact name, handled separately)
}

/** Exact filename matches for config/special files. */
const NAME_MAP: Record<string, FileIconInfo> = {
  'dockerfile':      { icon: FileCog, color: 'text-blue-400' },
  'dockerfile.dev':  { icon: FileCog, color: 'text-blue-400' },
  'dockerfile.prod': { icon: FileCog, color: 'text-blue-400' },
  'makefile':        { icon: FileCog, color: 'text-green-400' },
  'cmakelists.txt':  { icon: FileCog, color: 'text-blue-400' },
  '.gitignore':      { icon: FileCog, color: 'text-orange-400' },
  '.gitattributes':  { icon: FileCog, color: 'text-orange-400' },
  '.env':            { icon: FileLock, color: 'text-yellow-400' },
  '.env.local':      { icon: FileLock, color: 'text-yellow-400' },
  '.env.production': { icon: FileLock, color: 'text-yellow-400' },
  '.env.development':{ icon: FileLock, color: 'text-yellow-400' },
  'license':         { icon: FileText, color: 'text-muted-foreground' },
  'readme':          { icon: FileText, color: 'text-blue-400' },
  'package.json':    { icon: FileJson, color: 'text-yellow-400' },
  'package-lock.json': { icon: FileLock, color: 'text-yellow-400' },
  'tsconfig.json':   { icon: FileJson, color: 'text-blue-400' },
  'vite.config.ts':  { icon: FileCog, color: 'text-purple-400' },
  'vite.config.js':  { icon: FileCog, color: 'text-purple-400' },
  'go.mod':          { icon: FileCog, color: 'text-cyan-400' },
  'go.sum':          { icon: FileLock, color: 'text-cyan-400' },
  'cargo.toml':      { icon: FileCog, color: 'text-orange-400' },
  'cargo.lock':      { icon: FileLock, color: 'text-orange-400' },
}

/** Default icon for unrecognized file extensions. */
const DEFAULT_ICON: FileIconInfo = {
  icon: FileType,
  color: 'text-muted-foreground',
}

/**
 * Resolves the icon and color for a file based on its name/extension.
 * Checks exact filename matches first (e.g. "Dockerfile", "package.json"),
 * then falls back to extension lookup. Returns a default file icon if no
 * match is found.
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
