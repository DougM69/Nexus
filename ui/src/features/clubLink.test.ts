// The club link's words, state by state (`clubLink.ts`): what a joining laptop and a host say
// about the link, whatever the Field Day mode switch says. Each case pins the words by value.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { CLUB_DEAD_SECS, CLUB_PING_SECS, clubLinkLines, FIND_CLUB_EVENTS_SECS, type ClubLinkLine } from './clubLink'
import type { ClubHosting, ClubLink } from '../types'

const JOINER: ClubLink = {
  role: 'position',
  addr: '192.168.1.10:42073',
  state: 'looking',
  downSinceUnix: 0,
  event: '',
  hostCall: '',
  position: 'CW tent',
  clubCode: 'ABCD-EFGH',
}

const HOSTING: ClubHosting = {
  up: true,
  port: 42073,
  addrs: [
    { addr: '192.168.1.10:42073', adapter: 'Wi-Fi' },
    { addr: '10.0.0.5:42073', adapter: '' },
  ],
  connected: 1,
}

const HOST: ClubLink = { ...JOINER, role: 'host', addr: '127.0.0.1:42073', state: 'joined', hosting: HOSTING }

/** Each line as [words, alert], with its addresses or code where it has them. */
function said(link: ClubLink, inClubBlock = false): unknown[] {
  return clubLinkLines(link, inClubBlock).map((l: ClubLinkLine) => [
    l.text,
    l.alert,
    ...(l.addrs ? [l.addrs] : []),
    ...(l.code ? [l.code] : []),
  ])
}

const NO_ANSWER =
  'Nothing answered: check that both laptops are on the same network, that Nexus on the host has Host a club event on, and, if the host runs Windows, that its firewall lets Nexus in: press Allow Nexus through Windows Firewall in Settings ▸ Contesting on the host.'
const REFUSED =
  'Nothing is listening there: check that Nexus on that laptop has Host a club event on, and that the port is the one its screen shows.'

describe("a joining laptop's link", () => {
  it('is looking until the address answers', () => {
    expect(said(JOINER)).toEqual([['Looking for the host at 192.168.1.10:42073…', false]])
  })

  it('says it cannot reach the host, and what to check for each way an attempt fails', () => {
    expect(said({ ...JOINER, state: 'unreachable', why: 'noAnswer' })).toEqual([
      ['Cannot reach the host at 192.168.1.10:42073. Trying again.', true],
      [NO_ANSWER, true],
    ])
    expect(said({ ...JOINER, state: 'unreachable', why: 'refused' })).toEqual([
      ['Cannot reach the host at 192.168.1.10:42073. Trying again.', true],
      [REFUSED, true],
    ])
    expect(said({ ...JOINER, addr: '192.168.1.10:99999', state: 'unreachable', why: 'badAddress' })).toEqual([
      [
        '192.168.1.10:99999 is not an address to join. Type the one the host shows after “Positions join at”, such as 192.168.1.10:42073.',
        true,
      ],
    ])
  })

  it('quotes the host when it is turned away, unless the club block beside it already does', () => {
    const away: ClubLink = { ...JOINER, state: 'turnedAway', told: 'this club is on the air as W9AAA' }
    expect(said(away)).toEqual([
      ['Turned away by the host at 192.168.1.10:42073: this club is on the air as W9AAA', true],
    ])
    expect(said(away, true)).toEqual([])
  })

  it('names the event, the host, this laptop’s position and its code once in', () => {
    const joined: ClubLink = { ...JOINER, state: 'joined', event: 'ILQP rehearsal', hostCall: 'W9AAA' }
    expect(said(joined)).toEqual([['Joined ILQP rehearsal, hosted by W9AAA, as CW tent.', false, 'ABCD-EFGH']])
    // The club block names the event, the host and the code: only the position is news.
    expect(said(joined, true)).toEqual([['This laptop is CW tent in this event.', false]])
  })

  it('says the host is lost and it is trying again, with why the attempt since failed', () => {
    const lost: ClubLink = { ...JOINER, state: 'lost', downSinceUnix: 200, event: 'ILQP rehearsal' }
    expect(said(lost)).toEqual([['Lost the host at 192.168.1.10:42073. Trying again.', true]])
    expect(said({ ...lost, why: 'noAnswer' })).toEqual([
      ['Lost the host at 192.168.1.10:42073. Trying again.', true],
      [NO_ANSWER, true],
    ])
  })
})

describe("a joined laptop's link, in the words the host's board uses", () => {
  const IN: ClubLink = { ...JOINER, state: 'joined', event: 'ILQP rehearsal', hostCall: 'W9AAA' }

  it('says Connected beside the position, with when the host was last heard', () => {
    const connected: ClubLink = { ...IN, status: 'connected', heardSecs: 3 }
    expect(said(connected, true)).toEqual([
      ['This laptop is CW tent in this event.', false],
      ['Connected to the host, heard 3 s ago.', false],
    ])
    expect(said(connected)).toEqual([
      ['Joined ILQP rehearsal, hosted by W9AAA, as CW tent.', false, 'ABCD-EFGH'],
      ['Connected to the host, heard 3 s ago.', false],
    ])
  })

  it('says Reconnecting, not In, once the host has missed its heartbeats, and why an attempt failed', () => {
    expect(said({ ...IN, status: 'reconnecting', heardSecs: 12 }, true)).toEqual([
      ['Reconnecting to the host at 192.168.1.10:42073: last heard 12 s ago.', true],
    ])
    // The link went down and the next attempt is under way: the same words, not "Lost" yet.
    expect(said({ ...IN, state: 'lost', why: 'noAnswer', status: 'reconnecting', heardSecs: 14 })).toEqual([
      ['Reconnecting to the host at 192.168.1.10:42073: last heard 14 s ago.', true],
      [NO_ANSWER, true],
    ])
    expect(said({ ...IN, state: 'looking', status: 'reconnecting', heardSecs: 9 })).toEqual([
      ['Reconnecting to the host at 192.168.1.10:42073: last heard 9 s ago.', true],
    ])
  })

  it('says Lost past the dead timeout, with when the host was last heard, in minutes and hours', () => {
    expect(said({ ...IN, state: 'lost', status: 'lost', heardSecs: 16 }, true)).toEqual([
      ['Lost the host at 192.168.1.10:42073, last heard 16 s ago. Trying again.', true],
    ])
    expect(said({ ...IN, state: 'lost', status: 'lost', heardSecs: 4 * 60 + 59 })[0]).toEqual([
      'Lost the host at 192.168.1.10:42073, last heard 4 min ago. Trying again.',
      true,
    ])
    expect(said({ ...IN, state: 'lost', status: 'lost', heardSecs: 2 * 3600 + 5 })[0]).toEqual([
      'Lost the host at 192.168.1.10:42073, last heard 2 h ago. Trying again.',
      true,
    ])
  })

  it('CONTROL: turned away, the host’s own words stand alone, whatever was heard before', () => {
    const away: ClubLink = { ...IN, state: 'turnedAway', told: 'the host removed this laptop' }
    expect(said(away)).toEqual([['Turned away by the host at 192.168.1.10:42073: the host removed this laptop', true]])
    expect(said(away, true)).toEqual([])
  })
})

describe("the host's line", () => {
  it('says where a position joins, and how many others are in', () => {
    expect(said(HOST)).toEqual([
      ['Hosting on port 42073. Positions join at:', false, ['192.168.1.10:42073 (Wi-Fi)', '10.0.0.5:42073']],
      ['1 other position connected.', false],
    ])
    expect(said({ ...HOST, hosting: { ...HOSTING, connected: 3 } })[1]).toEqual(['3 other positions connected.', false])
    expect(said({ ...HOST, hosting: { ...HOSTING, addrs: [] } })[0]).toEqual(['Hosting on port 42073.', false])
  })

  it('says why it is not hosting, or that it is opening the port', () => {
    const down = (h: Partial<ClubHosting>) => said({ ...HOST, state: 'looking', hosting: { ...HOSTING, up: false, ...h } })
    expect(down({ failed: 'portInUse' })).toEqual([
      [
        'Not hosting: another program on this laptop is using port 42073. Close it, or set another Host port under Field Day Club Sync in Settings ▸ Contesting, and positions join at that port.',
        true,
      ],
    ])
    expect(down({ failed: 'port', detail: 'access denied' })).toEqual([
      [
        'Not hosting: port 42073 could not be opened (access denied). Set another Host port under Field Day Club Sync in Settings ▸ Contesting.',
        true,
      ],
    ])
    expect(down({ failed: 'start', detail: 'club sync cannot run CQ WW' })).toEqual([
      ['Not hosting: club sync cannot run CQ WW', true],
    ])
    expect(down({})).toEqual([['Opening port 42073 for the club event…', false]])
  })

  it('says nothing of its own loopback join but a refusal', () => {
    const lines = said({ ...HOST, state: 'unreachable', why: 'refused' })
    expect(lines).toHaveLength(2)
    expect(said({ ...HOST, state: 'turnedAway', told: 'another laptop holds this position' })[2]).toEqual([
      'Turned away by the host at 127.0.0.1:42073: another laptop holds this position',
      true,
    ])
  })
})

describe("contacts logged on another call than the club's", () => {
  const ONE =
    "1 contact was logged on another call than the club's, so it stays in this laptop's own log and is not sent to the club."
  const TWO =
    "2 contacts were logged on another call than the club's, so they stay in this laptop's own log and are not sent to the club."
  const IN: ClubLink = { ...JOINER, state: 'joined', event: 'ILQP', hostCall: 'W9XYZ', status: 'connected', heardSecs: 3 }

  it('are said last, on a joined laptop and on the host', () => {
    expect(said({ ...IN, otherCall: 1 }, true)).toEqual([
      ['This laptop is CW tent in this event.', false],
      ['Connected to the host, heard 3 s ago.', false],
      [ONE, true],
    ])
    expect(said({ ...HOST, otherCall: 2 })).toEqual([...said(HOST), [TWO, true]])
  })

  it('CONTROL: none, nothing said', () => {
    expect(said({ ...IN, otherCall: 0 }, true)).toEqual(said(IN, true))
    expect(said(IN, true)).toHaveLength(2)
  })
})

describe('Find club events', () => {
  it('says the time the search listens for', () => {
    const fdsync = readFileSync(
      fileURLToPath(new URL('../../../crates/tempo-net/src/fdsync.rs', import.meta.url)),
      'utf8',
    )
    const secs = /pub const DISCOVER_SECS: u64 = (\d+);/.exec(fdsync)?.[1]
    expect(secs, 'the search time in fdsync.rs').toBeDefined()
    expect(FIND_CLUB_EVENTS_SECS).toBe(Number(secs))
  })
})

describe("the link words' thresholds", () => {
  it('are the heartbeat and the dead timeout the club link runs on', () => {
    const fdsync = readFileSync(
      fileURLToPath(new URL('../../../crates/tempo-net/src/fdsync.rs', import.meta.url)),
      'utf8',
    )
    const ping = /pub const PING_SECS: u64 = (\d+);/.exec(fdsync)?.[1]
    const dead = /pub const DEAD_SECS: u64 = (\d+);/.exec(fdsync)?.[1]
    expect([ping, dead], 'the two in fdsync.rs').not.toContain(undefined)
    expect([CLUB_PING_SECS, CLUB_DEAD_SECS]).toEqual([Number(ping), Number(dead)])
  })
})
