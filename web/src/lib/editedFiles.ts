import type { AppEvent, EditedFile } from '@/types'

/** Aggregate `FileWritten` events into a deduplicated list of edited files.
 *  The latest write per path wins (highest event ID). Returns paths sorted
 *  alphabetically for stable output. Pure function — no side effects. */
export function aggregateEditedFiles(events: AppEvent[]): EditedFile[] {
  const latest = new Map<string, AppEvent>()

  for (const event of events) {
    if (event.type !== 'FileWritten') continue

    const path = event.target
    if (!path) continue

    const previous = latest.get(path)
    if (!previous || (event.id ?? 0) > (previous.id ?? 0)) {
      latest.set(path, event)
    }
  }

  return Array.from(latest.entries())
    .map(([path, event]) => ({
      path,
      eventId: event.id ?? 0,
      // AppEvent does not carry a timestamp. The backend endpoint provides it;
      // WS-stream aggregation is used only for the path list and change signal.
      timestamp: '',
    }))
    .sort((a, b) => a.path.localeCompare(b.path))
}
