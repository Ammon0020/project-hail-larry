# DOCX and XLSX rendered via dangerouslySetInnerHTML with no sanitization layer

- **Difficulty:** medium
- **Urgency:** low
- **File:** `web/src/components/FileViewer.tsx`
- **Lines:** 343-353 (DOCX), 418-422 (XLSX)

## Description

`DocxViewer` calls `mammoth.convertToHtml({ arrayBuffer: buf })` and injects the result with `dangerouslySetInnerHTML={{ __html: html }}` (`FileViewer.tsx:349`). `XlsxViewer` calls `XLSX.utils.sheet_to_html(sheet, { editable: false })` and injects with `dangerouslySetInnerHTML` (`FileViewer.tsx:420`). Both libraries escape cell/run text by default, so today this does not execute attacker-controlled HTML. However, there is no sanitization layer (no `dompurify`/`sanitize-html`) between the library output and the DOM, so the safety depends entirely on mammoth's and SheetJS's continued escaping behavior. A crafted `.docx`/`.xlsx` placed in the workspace by an agent (or pre-existing) that triggers a library bug or an unescaped code path would execute script in the IDE origin and steal the localStorage device secret (device-credential-localstorage). This is a defense-in-depth gap, not a confirmed exploit.

## Recommendation

Pipe both HTML strings through `DOMPurify.sanitize(html, { USE_PROFILES: { html: true } })` before assigning to `__html`. Alternatively, render the converted HTML inside a sandboxed iframe (`sandbox=""`) so even an escape cannot reach the IDE origin.

## Verification

`FileViewer.tsx:318` `import('mammoth').then((m) => m.convertToHtml({ arrayBuffer: buf }))`; `FileViewer.tsx:349` `dangerouslySetInnerHTML={{ __html: html }}` with no `DOMPurify`/`sanitize` call between them. `FileViewer.tsx:371` `XLSX.utils.sheet_to_html(sheet, { editable: false })`; `FileViewer.tsx:420` `dangerouslySetInnerHTML={{ __html: sheets[activeSheet]?.html || '' }}`. `package.json` has no `dompurify`/`sanitize-html`/`xss` dependency.
