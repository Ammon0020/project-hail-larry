import { useState } from 'react'
import { Search } from 'lucide-react'

/**
 * Search panel — workspace-wide search (Blueprint Sec 17 — left sidebar).
 *
 * STATUS: stub. The input is wired to local state so the control is
 * interactive, but no backend search endpoint exists yet — see
 * docs/STATUS.md "File search". Until the backend lands, the panel shows a
 * "coming soon" empty state instead of fake hardcoded results (AGENTS.md —
 * mark gaps honestly).
 */
export function SearchPanel() {
  const [query, setQuery] = useState('')

  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-2 text-[10px] font-semibold text-gray-500 uppercase tracking-wider shrink-0">
        Search
      </div>
      <div className="px-3 pb-2 shrink-0">
        <label htmlFor="search-panel-input" className="sr-only">Search files</label>
        <div className="relative">
          <Search className="w-3.5 h-3.5 text-gray-500 absolute left-2.5 top-1/2 -translate-y-1/2" />
          <input
            id="search-panel-input"
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search files..."
            className="w-full bg-background border border-gray-700 rounded-md pl-8 pr-3 py-1.5 text-xs focus:outline-none focus:border-blue-500 transition"
            aria-label="Search files"
          />
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-3 pb-2 text-xs">
        {query.trim() ? (
          <div className="text-gray-500 text-center py-6">
            File search is not yet available.
          </div>
        ) : (
          <div className="text-gray-500 text-center py-6">
            Type to search across the workspace.
          </div>
        )}
      </div>
    </div>
  )
}
