# Markdown Prose Classes Duplicated Across Chat Components

## Location
- [ChatMessageItem.tsx:135](file:///media/adam/extex/projects/project-hail-larry/web/src/components/ChatMessageItem.tsx#L135) — `userBubble` cva definition
- [ChatMessageItem.tsx:358](file:///media/adam/extex/projects/project-hail-larry/web/src/components/ChatMessageItem.tsx#L358) — `StreamUpdate` agent message

## Problem

The same long Tailwind class string for markdown prose styling is duplicated between the user bubble (`userBubble` cva) and the agent response (`StreamUpdate` case):

```
prose prose-sm prose-invert max-w-none [&_pre]:bg-tool-call [&_pre]:rounded-md
[&_pre]:border [&_pre]:border-border [&_pre]:p-2 [&_pre]:text-xs
[&_pre]:overflow-x-auto [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5
[&_code]:rounded [&_code]:text-xs [&_a]:text-primary [&_a]:hover:underline
```

This ~200-character class string appears in both locations nearly identically.

## Impact

- Changing the prose styling (e.g. code block background, link color) requires updating two locations.
- The strings can drift — one could be updated and the other forgotten.

## Suggested Fix

Per AGENTS.md Tailwind standards: *"Extract repeated class patterns into components or `cva` variants"*.

Extract a shared `proseClasses` constant or a `MarkdownContent` wrapper component:

```tsx
const proseClasses = 'prose prose-sm prose-invert max-w-none ...'

// Or even better, a component:
function MarkdownContent({ children }: { children: string }) {
  return (
    <div className={proseClasses}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
    </div>
  )
}
```
