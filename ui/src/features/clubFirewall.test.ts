// The club section's Windows Firewall words (`clubFirewall.ts`): where Nexus's club sync rules
// stand, what else keeps the club out, and what a press came to. Each case pins the words by value.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  CLUB_SEARCH_PORT,
  clubFirewallLines,
  clubFirewallPressLine,
  clubFirewallRefusalLine,
} from './clubFirewall'
import type { ClubFirewall } from '../types'

const IN_PLACE: ClubFirewall = {
  state: 'inPlace',
  ours: true,
  blockedPrivate: false,
  blockedPublic: false,
  blocksAllPrivate: false,
  blocksAllPublic: false,
  hostPort: 42073,
  searchPort: 42074,
  boardPort: 7373,
}

/** Each line as [key, words, alert]. */
const said = (fw: ClubFirewall | null) => clubFirewallLines(fw).map((l) => [l.key, l.text, l.alert])

const BLOCKED = (where: string, rule = 'a rule', it = 'it') =>
  `Windows also blocks Nexus on ${where} networks with ${rule} of its own (what Cancel at its question leaves), and a block wins over every allow. Delete ${it} in Windows Defender Firewall ▸ Advanced settings ▸ Inbound Rules: the Nexus rules with a block sign.`

const read = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')

describe("where Nexus's rules stand", () => {
  it('says each state', () => {
    expect(said(IN_PLACE)).toEqual([
      [
        'state',
        "Nexus's club sync rules are in Windows Firewall for Private and Public networks: TCP 42073, UDP 42074 and TCP 7373.",
        false,
      ],
    ])
    expect(said({ ...IN_PLACE, state: 'inPlace', hostPort: 50000, boardPort: 7474 })[0][1]).toBe(
      "Nexus's club sync rules are in Windows Firewall for Private and Public networks: TCP 50000, UDP 42074 and TCP 7474.",
    )
    expect(said({ ...IN_PLACE, state: 'missing', ours: false })).toEqual([
      ['state', 'Nexus has no club sync rule in Windows Firewall on this laptop yet.', false],
    ])
    expect(said({ ...IN_PLACE, state: 'stale' })).toEqual([
      [
        'state',
        "Nexus's club sync rules in Windows Firewall are out of date (made for another port, or another copy of Nexus). Press Allow Nexus through Windows Firewall again.",
        true,
      ],
    ])
    expect(said({ ...IN_PLACE, state: 'off' })).toEqual([
      [
        'state',
        'Windows Firewall is off on this laptop for Private and Public networks, so there is nothing to allow.',
        false,
      ],
    ])
    expect(said(null)).toEqual([['state', 'Nexus could not read Windows Firewall on this laptop.', false]])
  })

  it('says what still keeps the club out, on the kinds of network it does', () => {
    expect(said({ ...IN_PLACE, blockedPublic: true }).slice(1)).toEqual([['blocked', BLOCKED('Public'), true]])
    expect(said({ ...IN_PLACE, blockedPrivate: true }).slice(1)).toEqual([['blocked', BLOCKED('Private'), true]])
    expect(said({ ...IN_PLACE, blockedPrivate: true, blockedPublic: true }).slice(1)).toEqual([
      ['blocked', BLOCKED('Private and Public', 'rules', 'them'), true],
    ])
    expect(said({ ...IN_PLACE, blocksAllPublic: true }).slice(1)).toEqual([
      [
        'blocksAll',
        'Windows Firewall blocks every incoming connection on Public networks, whatever is allowed. Turn that off under Windows Security ▸ Firewall & network protection ▸ Public network.',
        true,
      ],
    ])
    expect(said({ ...IN_PLACE, blocksAllPrivate: true }).slice(1)).toEqual([
      [
        'blocksAll',
        'Windows Firewall blocks every incoming connection on Private networks, whatever is allowed. Turn that off under Windows Security ▸ Firewall & network protection ▸ Private network.',
        true,
      ],
    ])
    expect(said({ ...IN_PLACE, blocksAllPrivate: true, blocksAllPublic: true }).slice(1)).toEqual([
      [
        'blocksAll',
        'Windows Firewall blocks every incoming connection on Private and Public networks, whatever is allowed. Turn that off for each under Windows Security ▸ Firewall & network protection.',
        true,
      ],
    ])
    // Both at once, in that order; CONTROL: nothing in the way says nothing more.
    expect(said({ ...IN_PLACE, blockedPublic: true, blocksAllPublic: true }).map((l) => l[0])).toEqual([
      'state',
      'blocked',
      'blocksAll',
    ])
    expect(said(IN_PLACE)).toHaveLength(1)
  })
})

describe('a press', () => {
  it('says nothing when it did what it asked, and why not when the read-back says otherwise', () => {
    const allowFailed =
      "Windows did not add Nexus's rules: its administrator prompt was declined, or a step failed."
    const removeFailed =
      "Windows did not remove Nexus's rules: its administrator prompt was declined, or a step failed."
    expect(clubFirewallPressLine('allow', { ran: true, status: IN_PLACE })).toBeNull()
    expect(clubFirewallPressLine('allow', { ran: false, status: { ...IN_PLACE, state: 'missing', ours: false } })).toBe(
      allowFailed,
    )
    // The rules read back are the truth, whatever the command said.
    expect(clubFirewallPressLine('allow', { ran: true, status: { ...IN_PLACE, state: 'stale' } })).toBe(allowFailed)
    expect(clubFirewallPressLine('allow', { ran: false, status: IN_PLACE })).toBeNull()
    expect(clubFirewallPressLine('remove', { ran: true, status: { ...IN_PLACE, state: 'missing', ours: false } })).toBeNull()
    expect(clubFirewallPressLine('remove', { ran: false, status: IN_PLACE })).toBe(removeFailed)
    // Nothing read back: the command's own answer.
    expect(clubFirewallPressLine('allow', { ran: true, status: null })).toBeNull()
    expect(clubFirewallPressLine('remove', { ran: false, status: null })).toBe(removeFailed)
  })

  it('names each refusal', () => {
    expect(clubFirewallRefusalLine('onAir')).toBe(
      "Not while transmitting: Windows' administrator prompt takes the whole screen, Stop with it. Press again when the over ends.",
    )
    expect(clubFirewallRefusalLine('running')).toBe('Windows is still asking about the last press.')
    expect(clubFirewallRefusalLine('path')).toBe(
      "Nexus cannot name its own program in a rule from the folder it runs from (a % in its name, or a character Windows' command line would misread). Allow Nexus by hand under Windows Security ▸ Firewall & network protection ▸ Allow an app through firewall, on Private and Public networks.",
    )
    expect(clubFirewallRefusalLine('port')).toBe(
      "A rule cannot name port 0: set the Host port and the scoreboard's port first.",
    )
    expect(clubFirewallRefusalLine('firewall press failed: boom')).toBe(
      'Nexus could not ask Windows: firewall press failed: boom',
    )
  })
})

describe('the shell holds the same numbers and names', () => {
  it("names Find club events' port as fdsync.rs does", () => {
    const port = /pub const BEACON_PORT: u16 = (\d+);/.exec(read('../../../crates/tempo-net/src/fdsync.rs'))?.[1]
    expect(port, 'the beacon port in fdsync.rs').toBeDefined()
    expect(CLUB_SEARCH_PORT).toBe(Number(port))
  })

  it("reads every field fdfirewall.rs's Status sends", () => {
    const rs = read('../../../crates/tempo-net/src/fdfirewall.rs')
    const body = rs.split('pub struct Status {')[1]?.split('\n}')[0] ?? ''
    const fields = [...body.matchAll(/^\s*pub (\w+):/gm)].map((m) => m[1])
    expect(fields.length, 'the parser found the struct').toBeGreaterThan(5)
    const camel = (snake: string) => snake.replace(/_(\w)/g, (_, c: string) => c.toUpperCase())
    expect(fields.map(camel).sort()).toEqual(Object.keys(IN_PLACE).sort())
    // CONTROL: one field fewer is seen.
    expect(fields.slice(1).map(camel).sort()).not.toEqual(Object.keys(IN_PLACE).sort())
  })
})
