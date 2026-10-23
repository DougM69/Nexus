/**
 * This laptop's club link in words (`AppSnapshot.clubLink`, `Engine::fd_club_link`): where its
 * join stands (looking, in, turned away, not reached, lost) and, on the host, the hosting line
 * with the addresses a joining laptop types. Settings' club section and the Contest screen both
 * show these lines, whatever the Field Day mode switch says: at a rehearsal a laptop typed the
 * host's address and nothing on it said whether it was connected.
 *
 * Words only, so each screen renders them in its own style. Every key is spelled out at its
 * call, which is how the catalog guards can see it.
 */
import { t } from '../i18n'
import type { ClubLink } from '../types'

/** How long "Find club events" listens, in seconds: `tempo_net::fdsync::DISCOVER_SECS`, which
 *  the shell's search uses (a test holds the two together). */
export const FIND_CLUB_EVENTS_SECS = 5

/** The club link's heartbeat and dead timeout, in seconds: `tempo_net::fdsync::PING_SECS` and
 *  `DEAD_SECS`, which the host's board measures its link words by (`fdevent::PositionLink`): a
 *  position is Connected while heard within two heartbeats, Reconnecting until the dead timeout,
 *  and Lost past it. Said in the board's tooltip; a test holds them to fdsync.rs. */
export const CLUB_PING_SECS = 5
export const CLUB_DEAD_SECS = 15

/** Between two of the addresses on the host's line: punctuation, not prose. */
export const ADDR_SEP = ' · '

/** The join address the words show as an example: a wire format, never translated. */
const JOIN_EXAMPLE = '192.168.1.10:42073'

/** One line of the club link: its words, and whether it reports something wrong. */
export interface ClubLinkLine {
  key: string
  text: string
  alert: boolean
  /** The host's line: the addresses a joining laptop types, shown after the words. */
  addrs?: string[]
  /** The joined line: this laptop's club code, shown after the words. */
  code?: string
}

/** How long ago a club laptop was last heard, in seconds, minutes or hours: "12 s ago", "4 min
 *  ago". The host's board and a position's own link line both say it this way. */
export function heardAgo(secs: number): string {
  if (secs < 60) return t('fieldDay.club.heard.secs', { secs })
  if (secs < 3600) return t('fieldDay.club.heard.mins', { mins: Math.floor(secs / 60) })
  return t('fieldDay.club.heard.hours', { hours: Math.floor(secs / 3600) })
}

/** What a failed attempt to reach the host leaves to check there. */
function whyLine(why: ClubLink['why']): ClubLinkLine | null {
  switch (why) {
    case 'refused':
      return { key: 'why', text: t('fieldDay.club.link.why.refused'), alert: true }
    case 'noAnswer':
      return { key: 'why', text: t('fieldDay.club.link.why.noAnswer'), alert: true }
    default:
      return null
  }
}

/** The host's line: open and where to join it, opening, or why it is not hosting. */
function hostingLines(link: ClubLink): ClubLinkLine[] {
  const h = link.hosting
  if (!h) return []
  if (h.up) {
    const addrs = h.addrs.map((a) => (a.adapter ? `${a.addr} (${a.adapter})` : a.addr))
    return [
      addrs.length > 0
        ? { key: 'hosting', text: t('fieldDay.club.link.hosting', { port: h.port }), alert: false, addrs }
        : { key: 'hosting', text: t('fieldDay.club.link.hostingNoAddress', { port: h.port }), alert: false },
      { key: 'connected', text: t('fieldDay.club.link.connected', { count: h.connected }), alert: false },
    ]
  }
  switch (h.failed) {
    case 'portInUse':
      return [{ key: 'notHosting', text: t('fieldDay.club.link.notHosting.portInUse', { port: h.port }), alert: true }]
    case 'port':
      return [
        {
          key: 'notHosting',
          text: t('fieldDay.club.link.notHosting.port', { port: h.port, detail: h.detail ?? '' }),
          alert: true,
        },
      ]
    case 'start':
      return [{ key: 'notHosting', text: t('fieldDay.club.link.notHosting.start', { detail: h.detail ?? '' }), alert: true }]
    default:
      return [{ key: 'opening', text: t('fieldDay.club.link.opening', { port: h.port }), alert: false }]
  }
}

/**
 * The lines for `link`. `inClubBlock`: the Contest screen's club block is beside them, and it
 * already names the event, the host and the club code and quotes a refusal ("Host: …"), so
 * those are not said twice. The host's own position joins it over loopback, so of its link only
 * a refusal is news; its hosting line says the rest. Last, on the host as on any other laptop,
 * the contacts this laptop logged on another call than the club's, which the club chip does not
 * count: they stay in its own log.
 */
export function clubLinkLines(link: ClubLink, inClubBlock = false): ClubLinkLine[] {
  const lines = linkLines(link, inClubBlock)
  if (link.otherCall) {
    lines.push({ key: 'otherCall', text: t('fieldDay.club.link.otherCall', { count: link.otherCall }), alert: true })
  }
  return lines
}

function linkLines(link: ClubLink, inClubBlock: boolean): ClubLinkLine[] {
  const lines = hostingLines(link)
  const addr = link.addr
  // A position that has been in the event says its link in the host board's words: past the
  // heartbeats it says Reconnecting, and Lost past the dead timeout, whatever the socket is doing
  // between attempts; Connected keeps the line that says which position this laptop is.
  if (link.role === 'position' && link.status && link.status !== 'connected') {
    const ago = heardAgo(link.heardSecs ?? 0)
    lines.push(
      link.status === 'reconnecting'
        ? { key: 'link', text: t('fieldDay.club.link.reconnecting', { addr, ago }), alert: true }
        : { key: 'link', text: t('fieldDay.club.link.lostHeard', { addr, ago }), alert: true },
    )
    const why = whyLine(link.why)
    if (why) lines.push(why)
    return lines
  }
  switch (link.state) {
    case 'turnedAway':
      if (!inClubBlock) {
        lines.push({
          key: 'link',
          text: t('fieldDay.club.link.turnedAway', { addr, told: link.told ?? '' }),
          alert: true,
        })
      }
      return lines
    case 'joined':
      if (link.role === 'host') return lines
      lines.push(
        inClubBlock
          ? { key: 'link', text: t('fieldDay.club.link.youAre', { position: link.position }), alert: false }
          : {
              key: 'link',
              text: t('fieldDay.club.link.joined', { event: link.event, call: link.hostCall, position: link.position }),
              alert: false,
              code: link.clubCode || undefined,
            },
      )
      if (link.status === 'connected') {
        lines.push({
          key: 'heard',
          text: t('fieldDay.club.link.heard', { ago: heardAgo(link.heardSecs ?? 0) }),
          alert: false,
        })
      }
      return lines
    default:
      break
  }
  if (link.role === 'host') return lines
  if (link.state === 'looking') {
    lines.push({ key: 'link', text: t('fieldDay.club.link.looking', { addr }), alert: false })
    return lines
  }
  if (link.state === 'unreachable' && link.why === 'badAddress') {
    lines.push({
      key: 'link',
      text: t('fieldDay.club.link.badAddress', { addr, example: JOIN_EXAMPLE }),
      alert: true,
    })
    return lines
  }
  lines.push(
    link.state === 'lost'
      ? { key: 'link', text: t('fieldDay.club.link.lost', { addr }), alert: true }
      : { key: 'link', text: t('fieldDay.club.link.unreachable', { addr }), alert: true },
  )
  const why = whyLine(link.why)
  if (why) lines.push(why)
  return lines
}
