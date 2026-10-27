// The routing in plain words: what Nexus sends to each radio, read off the same three tiers the
// engine decides by (`Settings::route_radio` in settings.rs):
//   1. the routing rules, first match wins, a rule naming a disabled radio skipped;
//   2. band coverage: a radio that lists the band, else one with no list (a catch-all);
//   3. the default radio, for a band the active radio does not cover.
// Pure data in, data out, so the Radios card and the line above the rules list render the same
// thing and the tests below pin it without a screen.
import type { RadioProfile, RouteMode, RoutingRule } from '../types'

/**
 * The five routing mode classes, by their operator-facing names (the Settings rule editor's
 * dropdown, and the radio switch notice). These match the Rust `RouteMode::label`.
 *
 * ⚠️ NOT IN THE CATALOG, DELIBERATELY (i18n phase 2). These five are MODE NAMES — the same
 * invariant vocabulary that keeps the Phone / CW / Digital tabs untranslated — and this array's
 * own contract is that they match the Rust `RouteMode::label`, which no locale reaches.
 * Translating this copy alone would break that parity. The words around them (`Any mode`,
 * `Satellite`, the rule summary) ARE prose and live in `en.ts`.
 */
export const ROUTE_MODES: [RouteMode, string][] = [
  ['digital', 'Weak-signal digital'],
  ['fm', 'FM & APRS'],
  ['ssb', 'SSB phone'],
  ['cw', 'CW'],
  ['rtty', 'RTTY'],
]
export const ROUTE_MODE_LABEL: Record<RouteMode, string> = Object.fromEntries(ROUTE_MODES) as Record<
  RouteMode,
  string
>

export interface RoutingView {
  radios?: Pick<RadioProfile, 'id' | 'name' | 'bands' | 'enabled'>[]
  routingRules?: RoutingRule[]
  defaultRadio?: number | null
}

export type RouteLine =
  /** Rule `n` (1-based list position: first match wins) sends these bands and mode here. */
  | { kind: 'rule'; n: number; bands: string[]; mode: RouteMode | null; satellite: boolean }
  /** The radio lists these bands. */
  | { kind: 'bands'; bands: string[] }
  /** No band list: it takes a band no other radio lists. */
  | { kind: 'catchAll' }
  /** The default radio: a band the active radio does not cover and nothing else claims. */
  | { kind: 'default' }
  /** Disabled: the routing never switches to it. */
  | { kind: 'disabled' }

export function describeRadioRouting(view: RoutingView, id: number): RouteLine[] {
  const radio = (view.radios ?? []).find((r) => r.id === id)
  if (!radio) return []
  // A disabled radio is skipped by every tier (`usable` in route_radio, `enabled` in radio_for_band).
  if (radio.enabled === false) return [{ kind: 'disabled' }]
  const lines: RouteLine[] = []
  ;(view.routingRules ?? []).forEach((r, i) => {
    if (r.radio !== id) return
    lines.push({ kind: 'rule', n: i + 1, bands: r.bands, mode: r.mode, satellite: r.context === 'satellite' })
  })
  lines.push(radio.bands.length ? { kind: 'bands', bands: radio.bands } : { kind: 'catchAll' })
  if (view.defaultRadio === id) lines.push({ kind: 'default' })
  return lines
}
