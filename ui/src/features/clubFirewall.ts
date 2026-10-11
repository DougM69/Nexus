/**
 * The club section's Windows Firewall lines (`ClubFirewall`, `tempo_net::fdfirewall::status`):
 * where Nexus's club sync rules stand on this laptop, what else in Windows Firewall keeps the club
 * out, and what a press of Allow Nexus through Windows Firewall, or of its Remove, came to.
 *
 * Words only, so the section renders them in its own style. Every key is spelled out at its call,
 * which is how the catalog guards can see it.
 */
import { t } from '../i18n'
import type { ClubFirewall, ClubFirewallPress } from '../types'

/** Find club events' port: `tempo_net::fdsync::BEACON_PORT` (a test holds the two together). */
export const CLUB_SEARCH_PORT = 42074

/** One line: its words, and whether it reports something in the way. */
export interface ClubFirewallLine {
  key: string
  text: string
  alert: boolean
}

/** Windows' own rule blocking Nexus, on the kinds of network it blocks it on. */
function blockedLine(fw: ClubFirewall): ClubFirewallLine | null {
  const text =
    fw.blockedPrivate && fw.blockedPublic
      ? t('settings.fdClub.firewall.blocked.both')
      : fw.blockedPrivate
        ? t('settings.fdClub.firewall.blocked.private')
        : fw.blockedPublic
          ? t('settings.fdClub.firewall.blocked.public')
          : null
  return text ? { key: 'blocked', text, alert: true } : null
}

/** The firewall set to block every incoming connection, on the kinds of network it does. */
function blocksAllLine(fw: ClubFirewall): ClubFirewallLine | null {
  const text =
    fw.blocksAllPrivate && fw.blocksAllPublic
      ? t('settings.fdClub.firewall.blocksAll.both')
      : fw.blocksAllPrivate
        ? t('settings.fdClub.firewall.blocksAll.private')
        : fw.blocksAllPublic
          ? t('settings.fdClub.firewall.blocksAll.public')
          : null
  return text ? { key: 'blocksAll', text, alert: true } : null
}

/** Where Nexus's rules stand, then whatever in Windows Firewall still keeps the club out.
 *  `null`: the firewall could not be read. */
export function clubFirewallLines(fw: ClubFirewall | null): ClubFirewallLine[] {
  if (!fw) return [{ key: 'state', text: t('settings.fdClub.firewall.unread'), alert: false }]
  const lines: ClubFirewallLine[] = []
  switch (fw.state) {
    case 'off':
      lines.push({ key: 'state', text: t('settings.fdClub.firewall.off'), alert: false })
      break
    case 'missing':
      lines.push({ key: 'state', text: t('settings.fdClub.firewall.missing'), alert: false })
      break
    case 'stale':
      lines.push({ key: 'state', text: t('settings.fdClub.firewall.stale'), alert: true })
      break
    case 'inPlace':
      lines.push({
        key: 'state',
        text: t('settings.fdClub.firewall.inPlace', {
          host: fw.hostPort,
          search: fw.searchPort,
          board: fw.boardPort,
        }),
        alert: false,
      })
      break
  }
  for (const line of [blockedLine(fw), blocksAllLine(fw)]) {
    if (line) lines.push(line)
  }
  return lines
}

/** What a press came to, when it is not what was asked: the rules read back are the truth (all
 *  three in place after Allow, none of Nexus's after Remove), and the command's own answer only
 *  where nothing could be read back. `null` when the press did what it asked. */
export function clubFirewallPressLine(
  action: 'allow' | 'remove',
  outcome: ClubFirewallPress,
): string | null {
  const st = outcome.status
  const done = st ? (action === 'allow' ? st.state === 'inPlace' : !st.ours) : outcome.ran
  if (done) return null
  return action === 'allow'
    ? t('settings.fdClub.firewall.press.allowFailed')
    : t('settings.fdClub.firewall.press.removeFailed')
}

/** Why a press ran nothing (`club_firewall_press`'s refusal codes), or what went wrong asking. */
export function clubFirewallRefusalLine(code: string): string {
  switch (code) {
    case 'onAir':
      return t('settings.fdClub.firewall.press.onAir')
    case 'running':
      return t('settings.fdClub.firewall.press.running')
    case 'path':
      return t('settings.fdClub.firewall.press.path')
    case 'port':
      return t('settings.fdClub.firewall.press.port')
    default:
      return t('settings.fdClub.firewall.press.failed', { detail: code })
  }
}
