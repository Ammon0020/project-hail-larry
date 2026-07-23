# Tools whitelist input strips commas on every keystroke, blocking multi-tool entry

- **Difficulty:** medium
- **Urgency:** high
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ProfilesSettings.tsx`
- **Lines:** 445, 572-576

## Description

The tools whitelist input is a controlled input whose `value` is derived from `joinTools(entry.tools)` (line 445) and whose `onChange` runs `parseTools(e.target.value)` (line 575) and writes the normalized array back into the draft. `parseTools` (lines 56-60) splits on commas, trims, and `filter(Boolean)`s, which discards empty segments — including the trailing comma the user just typed. `joinTools` (line 64) then re-joins with `', '`.

Because this round-trip runs on every keystroke, a user typing `read_file,` sees the value snap back to `read_file` (comma removed) with the cursor at the end. Typing the next tool name produces `read_filewrite_file` instead of `read_file, write_file`, because the separator can never be entered. The only way to populate more than one tool is to paste a complete comma-separated string in a single event. Manual entry of a multi-tool whitelist — the input's stated purpose — is effectively impossible.

## Recommendation

Keep a local text-state buffer in `ProfileEditor` (e.g. `const [toolsText, setToolsText] = useState(joinTools(entry.tools))` synced via an effect when `entry.tools` or `id` changes) and only call `onChange({ tools: parseTools(text) })` on `blur` (or on a debounce). This lets the user type freely, including trailing commas and spaces, while the draft still ends up with a normalized `string[]`. Reset the local buffer when the selected profile id changes so switching profiles doesn't show the previous profile's text.

## Verification

Traced the round-trip: `toolsText = joinTools(entry.tools)` (line 445) and `onChange={e => onChange({ tools: parseTools(e.target.value) })}` (line 575). `parseTools` (lines 56-60) does `text.split(',').map(t => t.trim()).filter(Boolean)`, so `"read_file,"` → `["read_file"]` → `joinTools` → `"read_file"`. The trailing comma is removed on every change event, so a comma can never survive a keystroke.
