/**
 * Utilities for the context-usage ring around the chat send button.
 *
 * The ring visualizes how full the agent's context window is, based on ACP
 * `usage_update` events. These pure helpers keep the math testable without
 * coupling to React/DOM.
 */

/** Context usage data extracted from the latest `UsageUpdated` event. */
export interface ContextUsage {
  /** Tokens currently in context. */
  used: number
  /** Total context window size in tokens. */
  size: number
  /** Cumulative session cost amount (optional — agent may omit). */
  costAmount?: number
  /** ISO 4217 currency code for `costAmount` (e.g. "USD"). */
  costCurrency?: string
}

/**
 * Compute the fill fraction (0–1) of the context ring.
 *
 * Returns 0 when `size` is zero or missing (avoids division by zero and
 * hides the ring until the agent reports a window size). Clamps at 1 so a
 * misreported `used > size` doesn't overflow the ring.
 */
export function contextFillFraction(usage: ContextUsage | null): number {
  if (!usage || usage.size <= 0) return 0
  return Math.min(usage.used / usage.size, 1)
}

/**
 * Format the fill percentage for display, e.g. "53%".
 *
 * Rounds to the nearest integer. Returns "0%" when no usage is reported.
 */
export function contextFillPercent(usage: ContextUsage | null): string {
  const frac = contextFillFraction(usage)
  return `${Math.round(frac * 100)}%`
}

/**
 * Format a token count for compact display, e.g. "53k" or "200k".
 *
 * ACP `usage_update` reports raw token counts; the ring popout shows them
 * in thousands for readability.
 */
export function formatTokens(tokens: number): string {
  if (tokens >= 1000) {
    const k = tokens / 1000
    // Drop trailing .0 so "53.0k" → "53k" but "12.5k" stays.
    return `${k % 1 === 0 ? k.toFixed(0) : k.toFixed(1)}k`
  }
  return String(tokens)
}

/**
 * Format a cost amount with its currency code, e.g. "$0.05 USD".
 *
 * Returns an empty string when no cost is reported. Uses a simple currency
 * symbol map for common codes; falls back to the raw code prefix.
 */
export function formatCost(amount?: number | null, currency?: string): string {
  if (amount === undefined || amount === null) return ''
  const symbol = currencySymbol(currency)
  const prefix = symbol ? `${symbol}${amount.toFixed(2)}` : `${amount.toFixed(2)} ${currency ?? ''}`
  return prefix.trim()
}

/** Map common ISO 4217 codes to a display symbol. */
function currencySymbol(currency?: string): string {
  switch (currency?.toUpperCase()) {
    case 'USD':
      return '$'
    case 'EUR':
      return '€'
    case 'GBP':
      return '£'
    case 'JPY':
      return '¥'
    default:
      return ''
  }
}
