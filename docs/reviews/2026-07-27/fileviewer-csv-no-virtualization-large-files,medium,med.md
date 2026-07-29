- name: CSV viewer renders the entire file into the DOM — large CSVs jank or crash the tab
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/FileViewer.tsx
- lines: 493-543
- description: |
    `CsvViewer` fetches the whole file as text (line 500-501), parses it all
    into a `string[][]` in memory (line 504, `parseCsv`), and renders every
    row as a `<tr>`/`<td>` (line 530-538) with no virtualization or row cap.
    A multi-megabyte CSV (tens of thousands of rows) creates that many DOM
    nodes at once, which janks the main thread heavily or freezes/crashes the
    browser tab on mobile.

    There is also no truncation indicator — the user has no idea the file is
    too large to preview safely.

    Suggested fix: cap rendered rows (e.g. first 1000) with a "Showing 1000
      of N rows — download to view all" footer, and/or adopt a virtualized
      list for the table body. At minimum, guard `parseCsv` against pathologically
      large input (e.g. stop parsing after a byte/row limit) so the fetch
      itself doesn't OOM.
- verification: |
    Read FileViewer.tsx lines 493-543. `parseCsv` (545-582) returns the full
    `string[][]`; the render (530-538) maps over `rows` with no slice/cap and
    no virtualization. No row limit anywhere in the viewer.
