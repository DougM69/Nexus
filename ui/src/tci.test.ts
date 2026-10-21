// The TCI connection as the screens offer it (`tci.ts`).
//
// The dialect list must be the SETTINGS' list, not a second opinion: Rust reads `tci_dialect` with
// `TciDialect::from_setting` and Find TCI server tries `TciDialect::default_port`'s ports, so a
// screen that stored another word, or pre-filled another port, would hand the radio a dialect or a
// port nothing else in Nexus means. This reads the Rust source and fails if the two drift.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { TciProbeResult } from './api'
import {
  TCI_DEFAULT_HOST,
  TCI_DEFAULT_PORT,
  TCI_DIALECTS,
  TCI_TRX_CHOICES,
  tciDialectShown,
  tciPortForDialect,
  tciProbeMessage,
  tciServerName,
  tciTrxChoices,
} from './tci'

const RUST = fileURLToPath(new URL('../../crates/tempo-app/src/settings.rs', import.meta.url))

/** The body of the `pub const fn <name>` in `src`, up to its closing brace at four spaces. */
function constFn(src: string, name: string): string {
  const at = src.indexOf(`pub const fn ${name}(`)
  expect(at, `control: TciDialect::${name} was found`).toBeGreaterThan(0)
  return src.slice(at, src.indexOf('\n    }\n', at))
}

describe('the dialect list', () => {
  const src = readFileSync(RUST, 'utf8')

  it('stores the words TciDialect::as_setting reads, and pre-fills its default_port ports', () => {
    // `TciDialect::Auto => "auto",`
    const words = new Map(
      [...constFn(src, 'as_setting').matchAll(/TciDialect::(\w+) => "(\w+)"/g)].map((m) => [m[1], m[2]]),
    )
    // `TciDialect::Auto | TciDialect::ExpertSdr3 | TciDialect::Thetis => 50001,`
    const ports = new Map<string, number>()
    for (const m of constFn(src, 'default_port').matchAll(/((?:TciDialect::\w+(?: \| )?)+) => (\d+)/g)) {
      for (const v of m[1].matchAll(/TciDialect::(\w+)/g)) ports.set(v[1], Number(m[2]))
    }
    expect(words.size, 'control: every variant was parsed').toBe(4)
    expect(ports.size, 'control: every variant has a port').toBe(4)
    const rust = [...words].map(([variant, word]) => ({ value: word, port: ports.get(variant) }))
    expect(TCI_DIALECTS.map((d) => ({ value: d.value, port: d.port }))).toEqual(rust)
  })

  it('starts where Rust starts: Auto, this computer, Auto\'s port', () => {
    const host = /pub const TCI_DEFAULT_HOST: &str = "([^"]+)";/.exec(src)?.[1]
    expect(host, 'control: the Rust constant was found').toBeTruthy()
    expect(TCI_DEFAULT_HOST).toBe(host)
    expect(src).toContain('pub const TCI_DEFAULT_PORT: u16 = TciDialect::Auto.default_port();')
    expect(TCI_DIALECTS[0].value).toBe('auto')
    expect(TCI_DEFAULT_PORT).toBe(50001)
  })
})

describe('naming a program fills in its port', () => {
  it('over another program’s port, or none', () => {
    expect(tciPortForDialect(50001, 'pihpsdr')).toBe(40001)
    expect(tciPortForDialect(40001, 'thetis')).toBe(50001)
    expect(tciPortForDialect(40001, 'expertsdr3')).toBe(50001)
    expect(tciPortForDialect(0, 'pihpsdr')).toBe(40001)
  })

  it('never over a port the operator typed, and never for Automatic', () => {
    expect(tciPortForDialect(12345, 'pihpsdr')).toBe(12345)
    expect(tciPortForDialect(40001, 'auto'), 'Automatic says nothing about where it listens').toBe(40001)
    expect(tciPortForDialect(50001, 'not-a-dialect')).toBe(50001)
  })
})

describe('the choices show what the radio holds', () => {
  it('reads the stored dialect as Rust reads it', () => {
    expect(tciDialectShown('pihpsdr')).toBe('pihpsdr')
    expect(tciDialectShown('  PiHPSDR ')).toBe('pihpsdr')
    expect(tciDialectShown('a-newer-builds-word')).toBe('auto')
    expect(tciDialectShown(undefined)).toBe('auto')
  })

  it('offers the usual transceivers, and a stored one past them', () => {
    expect(tciTrxChoices(0)).toEqual([...Array(TCI_TRX_CHOICES).keys()])
    expect(tciTrxChoices(11)).toHaveLength(12)
    expect(tciTrxChoices(11)).toContain(11)
  })
})

const found = (over: Partial<NonNullable<TciProbeResult['found']>> = {}): TciProbeResult['found'] => ({
  port: 50001,
  program: 'ExpertSDR3',
  version: '2.0',
  device: 'SunSDR2DX',
  trxCount: 2,
  dialect: 'expertsdr3',
  ...over,
})

describe('the Find TCI server status line', () => {
  it('names what answered, as it said it, with its transceivers', () => {
    const r: TciProbeResult = { host: '127.0.0.1', resolved: true, tried: [{ port: 50001, outcome: 'found' }], found: found() }
    expect(tciProbeMessage(r)).toBe('Found a TCI server at 127.0.0.1:50001: ExpertSDR3 2.0, SunSDR2DX. It has 2 transceivers.')
    const one = { ...r, found: found({ trxCount: 1 }) }
    expect(tciProbeMessage(one)).toContain('It has 1 transceiver.')
  })

  it('says when it filled in another port, and why it skipped one', () => {
    const r: TciProbeResult = {
      host: '127.0.0.1',
      resolved: true,
      tried: [
        { port: 50001, outcome: 'nexus' },
        { port: 40001, outcome: 'found' },
      ],
      found: found({ port: 40001, device: 'SunSDR2QRP', trxCount: 1 }),
    }
    const said = tciProbeMessage(r, true)
    expect(said).toContain('Found a TCI server at 127.0.0.1:40001: ExpertSDR3 2.0, SunSDR2QRP.')
    expect(said).toContain('TCI port is now 40001: Save to keep it.')
    expect(said).toContain("Port 50001 is Nexus's own TCI server, not an SDR program.")
    // The control: with the port unchanged, nothing says it was filled in.
    expect(tciProbeMessage(r, false)).not.toContain('TCI port is now')
  })

  it('says when Nexus does not know the program', () => {
    const r: TciProbeResult = {
      host: '127.0.0.1',
      resolved: true,
      tried: [{ port: 50001, outcome: 'found' }],
      found: found({ program: 'MysterySDR', version: '9.9', device: null, trxCount: null, dialect: 'unknown' }),
    }
    const said = tciProbeMessage(r)
    expect(said).toContain('Found a TCI server at 127.0.0.1:50001: MysterySDR 9.9.')
    expect(said).toContain('Nexus does not know this program yet')
    expect(said).not.toContain('transceiver')
    expect(tciServerName(found({ program: null, version: null, device: null }) as never)).toBe('it does not give its name')
  })

  it('says what each port answered when nothing was found', () => {
    const r: TciProbeResult = {
      host: '192.0.2.7',
      resolved: true,
      tried: [
        { port: 50001, outcome: 'notTci' },
        { port: 40001, outcome: 'noReady' },
      ],
      found: null,
    }
    expect(tciProbeMessage(r)).toBe(
      'Something answered on port 50001, but it is not a TCI server. Port 40001 answered but did not finish the TCI handshake. ' +
        'No TCI server answered at 192.0.2.7 (ports tried: 50001, 40001). Start the SDR program, turn its TCI server on, and try again.',
    )
    expect(tciProbeMessage({ host: 'sdr-pc', resolved: false, tried: [], found: null })).toBe(
      'This computer cannot find sdr-pc. Check the SDR program address.',
    )
  })
})
