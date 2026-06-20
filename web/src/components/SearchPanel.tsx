import { Search, FileCode } from 'lucide-react'

/**
 * Search panel — workspace-wide search (Blueprint Sec 17 — left sidebar).
 * Shows search input and results grouped by file.
 */
export function SearchPanel() {
  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-2 text-[10px] font-semibold text-gray-500 uppercase tracking-wider shrink-0">
        Search
      </div>
      <div className="px-3 pb-2 shrink-0">
        <div className="relative">
          <Search className="w-3.5 h-3.5 text-gray-500 absolute left-2.5 top-1/2 -translate-y-1/2" />
          <input
            type="text"
            placeholder="Search files..."
            className="w-full bg-background border border-gray-700 rounded-md pl-8 pr-3 py-1.5 text-xs focus:outline-none focus:border-blue-500 transition"
          />
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-3 pb-2 text-xs space-y-3">
        <div>
          <div className="text-gray-500 font-mono mb-1">3 results in 2 files</div>
          <div className="flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-gray-800/50 text-gray-300">
            <FileCode className="w-3.5 h-3.5 text-yellow-400 shrink-0" /> server.js
          </div>
          <div className="ml-4 font-mono text-[11px] text-gray-400 space-y-0.5">
            <div className="hover:bg-gray-800/50 rounded px-1 cursor-pointer">
              <span className="text-gray-600">12:</span> app.<span className="text-blue-400">listen</span>(port, () =&gt; {'{'}
            </div>
            <div className="hover:bg-gray-800/50 rounded px-1 cursor-pointer">
              <span className="text-gray-600">28:</span> console.<span className="text-blue-400">log</span>(`Server on ${'{port}'}`);
            </div>
          </div>
          <div className="flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-gray-800/50 text-gray-300 mt-1">
            <FileCode className="w-3.5 h-3.5 text-yellow-400 shrink-0" /> routes/index.js
          </div>
          <div className="ml-4 font-mono text-[11px] text-gray-400 space-y-0.5">
            <div className="hover:bg-gray-800/50 rounded px-1 cursor-pointer">
              <span className="text-gray-600">7:</span> router.<span className="text-blue-400">get</span>('/', (req, res) =&gt; {'{'}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
