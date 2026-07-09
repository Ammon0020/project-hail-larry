# Audit summary table mislabels "Gap" items as "Deviation"

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `docs/reviews/2026-07-06/acp-audit.md`
- **Lines:** 63, 65 (table rows); 44, 46 (body Gap 2 / Gap 3)

## Description

The audit body lists "Terminal `env` parameter ignored" and "Terminal `signal` not captured" under the **Gaps** section (Gap 2 at line 44, Gap 3 at line 46). However, the summary table labels `terminal/create` as "Deviation" (line 63) and `terminal/wait_for_exit` as "Deviation" (line 65), while pointing to Gap 2 / Gap 3 in the notes. The audit defines three distinct categories (Correct / Deviation = "works but not idiomatic" / Gap = "missing or incomplete"), so labeling missing-feature gaps as deviations in the table contradicts the body's own categorization and muddies the distinction between "implemented unidiomatically" and "not implemented."

## Recommendation

Change the Status column for `terminal/create` and `terminal/wait_for_exit` rows from "Deviation" to "Gap" to match the body's categorization (or relabel the body items as Deviations if that is the intended semantics).

## Verification

Read audit.md lines 40-55 (Gaps section: Gap 2 env, Gap 3 signal) and lines 56-79 (summary table rows 63 and 65 labeled "Deviation" with notes referencing Gap 2/Gap 3).
