import { describe, it, expect } from 'vitest'
import { describeRadioRouting, type RoutingView } from './describeRouting'
import type { RadioProfile, RoutingRule } from '../types'

const radio = (id: number, name: string, bands: string[] = [], enabled = true) =>
  ({ id, name, bands, enabled }) as unknown as RadioProfile
const rule = (radioId: number, bands: string[], mode: RoutingRule['mode'], context?: 'satellite'): RoutingRule => ({
  bands,
  mode,
  context: context ?? null,
  radio: radioId,
})

// The operator's three-radio shack, as the Rust routing tests build it (`three_radio_engine`):
// FTdx10 catch-all and default, IC-9700 + FT-991A both listing 2 m / 70 cm, FM to the 991A and
// digital to the 9700 by rule.
const shack: RoutingView = {
  radios: [radio(0, 'FTdx10'), radio(1, 'IC-9700', ['2m', '70cm']), radio(2, 'FT-991A', ['2m', '70cm'])],
  routingRules: [rule(2, ['2m', '70cm'], 'fm'), rule(1, ['2m', '70cm'], 'digital')],
  defaultRadio: 0,
}

describe('describeRadioRouting: what the routing sends to a radio, in the order it decides', () => {
  it('names the rules that point at the radio, numbered by their place in the list', () => {
    expect(describeRadioRouting(shack, 2)).toEqual([
      { kind: 'rule', n: 1, bands: ['2m', '70cm'], mode: 'fm', satellite: false },
      { kind: 'bands', bands: ['2m', '70cm'] },
    ])
    expect(describeRadioRouting(shack, 1)).toEqual([
      { kind: 'rule', n: 2, bands: ['2m', '70cm'], mode: 'digital', satellite: false },
      { kind: 'bands', bands: ['2m', '70cm'] },
    ])
  })

  it('an empty band list takes any band no other radio lists, and the default takes everything else', () => {
    expect(describeRadioRouting(shack, 0)).toEqual([{ kind: 'catchAll' }, { kind: 'default' }])
  })

  it('a disabled radio is never switched to: no rule, list or default is described for it', () => {
    // route_radio skips a rule or default naming a disabled radio, and radio_for_band ranks only
    // enabled ones.
    const off: RoutingView = { ...shack, radios: shack.radios!.map((r) => (r.id === 2 ? { ...r, enabled: false } : r)) }
    expect(describeRadioRouting(off, 2)).toEqual([{ kind: 'disabled' }])
    const offDefault: RoutingView = { ...shack, radios: shack.radios!.map((r) => (r.id === 0 ? { ...r, enabled: false } : r)) }
    expect(describeRadioRouting(offDefault, 0)).toEqual([{ kind: 'disabled' }])
  })

  it('a satellite rule says it is for satellite passes; a rule with no bands or mode says any', () => {
    const view: RoutingView = {
      ...shack,
      routingRules: [rule(1, [], null, 'satellite'), rule(1, [], null)],
      defaultRadio: null,
    }
    expect(describeRadioRouting(view, 1)).toEqual([
      { kind: 'rule', n: 1, bands: [], mode: null, satellite: true },
      { kind: 'rule', n: 2, bands: [], mode: null, satellite: false },
      { kind: 'bands', bands: ['2m', '70cm'] },
    ])
  })

  it('a radio nothing points at, with no list, is still described (catch-all), never left blank', () => {
    const view: RoutingView = { radios: [radio(0, 'A', ['20m']), radio(1, 'B')], routingRules: [], defaultRadio: null }
    expect(describeRadioRouting(view, 1)).toEqual([{ kind: 'catchAll' }])
    expect(describeRadioRouting(view, 0)).toEqual([{ kind: 'bands', bands: ['20m'] }])
  })
})
