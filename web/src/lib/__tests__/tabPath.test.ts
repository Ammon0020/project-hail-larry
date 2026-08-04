import { describe, expect, it } from 'vitest'
import {
  joinUnderParent,
  joinWorkspacePath,
  pathIsUnder,
  previewTabId,
  remapAfterRename,
  remapTabIdAfterRename,
  tabIdTouchesPath,
} from '@/lib/tabPath'

describe('joinWorkspacePath', () => {
  it('uses the root separator and normalizes relative path separators', () => {
    expect(joinWorkspacePath('/workspace/', '\\src//main.ts')).toBe('/workspace/src/main.ts')
    expect(joinWorkspacePath('C:\\workspace\\', '/src\\main.ts')).toBe('C:\\workspace\\src\\main.ts')
  })

  it('removes trailing root separators and supports an empty relative path', () => {
    expect(joinWorkspacePath('/workspace///', '')).toBe('/workspace')
    expect(joinWorkspacePath('', '')).toBe('')
  })
})

describe('pathIsUnder', () => {
  it('matches an exact path or descendant across separators', () => {
    expect(pathIsUnder('src\\lib\\tabPath.ts', 'src/lib')).toBe(true)
    expect(pathIsUnder('src/lib/', 'src/lib')).toBe(true)
    expect(pathIsUnder('', '')).toBe(true)
  })

  it('does not match a similar sibling path', () => {
    expect(pathIsUnder('src/library.ts', 'src/lib')).toBe(false)
  })
})

describe('remapAfterRename', () => {
  it('remaps exact paths and descendants with normalized separators', () => {
    expect(remapAfterRename('src\\old', 'src/old', 'src/new')).toBe('src/new')
    expect(remapAfterRename('src\\old\\child.ts', 'src/old', 'src/new')).toBe('src/new/child.ts')
  })

  it('leaves unrelated and empty paths unchanged', () => {
    expect(remapAfterRename('src/older/file.ts', 'src/old', 'src/new')).toBe('src/older/file.ts')
    expect(remapAfterRename('', 'src/old', 'src/new')).toBe('')
  })
})

describe('previewTabId', () => {
  it('creates an id with the workspace and entry path', () => {
    expect(previewTabId('workspace-1', 'docs/readme.md')).toBe('preview:workspace-1:docs/readme.md')
  })
})

describe('joinUnderParent', () => {
  it('normalizes parent separators and removes a trailing slash', () => {
    expect(joinUnderParent('src\\lib/', '  tabPath.ts  ')).toBe('src/lib/tabPath.ts')
  })

  it('returns the trimmed name for an empty parent', () => {
    expect(joinUnderParent('', '  new-folder  ')).toBe('new-folder')
  })
})

describe('tabIdTouchesPath', () => {
  it('matches exact and descendant file tabs but not siblings', () => {
    expect(tabIdTouchesPath('src/lib', 'src/lib', 'workspace-1')).toBe(true)
    expect(tabIdTouchesPath('src\\lib\\tabPath.ts', 'src/lib', 'workspace-1')).toBe(true)
    expect(tabIdTouchesPath('src/library.ts', 'src/lib', 'workspace-1')).toBe(false)
  })

  it('matches only preview tabs for the active workspace and target path', () => {
    expect(tabIdTouchesPath('preview:workspace-1:docs/guide.md', 'docs', 'workspace-1')).toBe(true)
    expect(tabIdTouchesPath('preview:workspace-1:docs\\guide.md', 'docs', 'workspace-1')).toBe(true)
    expect(tabIdTouchesPath('preview:workspace-1:docs-other.md', 'docs', 'workspace-1')).toBe(false)
    expect(tabIdTouchesPath('preview:workspace-2:docs/guide.md', 'docs', 'workspace-1')).toBe(false)
    expect(tabIdTouchesPath('preview:workspace-1:docs/guide.md', 'docs', undefined)).toBe(false)
  })
})

describe('remapTabIdAfterRename', () => {
  it('remaps exact and descendant file tabs', () => {
    expect(remapTabIdAfterRename('src/old', 'src/old', 'src/new', 'workspace-1')).toBe('src/new')
    expect(remapTabIdAfterRename('src\\old\\child.ts', 'src/old', 'src/new', 'workspace-1')).toBe('src/new/child.ts')
  })

  it('remaps matching preview tabs for the active workspace', () => {
    expect(remapTabIdAfterRename('preview:workspace-1:src/old', 'src/old', 'src/new', 'workspace-1')).toBe(
      'preview:workspace-1:src/new',
    )
    expect(remapTabIdAfterRename('preview:workspace-1:src/old/child.ts', 'src/old', 'src/new', 'workspace-1')).toBe(
      'preview:workspace-1:src/new/child.ts',
    )
  })

  it('leaves unrelated, other-workspace, and unscoped preview tabs unchanged', () => {
    expect(remapTabIdAfterRename('src/older/file.ts', 'src/old', 'src/new', 'workspace-1')).toBe('src/older/file.ts')
    expect(remapTabIdAfterRename('preview:workspace-2:src/old/file.ts', 'src/old', 'src/new', 'workspace-1')).toBe(
      'preview:workspace-2:src/old/file.ts',
    )
    expect(remapTabIdAfterRename('preview:workspace-1:src/old/file.ts', 'src/old', 'src/new', undefined)).toBe(
      'preview:workspace-1:src/old/file.ts',
    )
  })
})
