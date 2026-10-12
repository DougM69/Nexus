// WHAT A CONTEST LOG LINE LOGS UNDER, AND THE MODE THE STATION IS ON — both read off the station's
// operating section (`snap.radio.operatingMode`, the engine's, which follows the operator from
// cockpit to cockpit). The large log line (`ContestLargeEntry`, in the contest logger window and at
// the top of the Contest screen) logs under the first; it and the Contest screen's banned-mode chip
// name the second, so the line and the chip cannot disagree about whether the contest permits it.
import { useEffect, useState } from 'react'
import { getPskState } from '../api'
import { PSK_MODE_BY_SLUG } from '../pskModes'

/** What a contest log line off the cockpits logs under: the station's operating section as the
 *  cockpit there logs it — Phone as PH, CW as CW, RTTY and the keyboard modes as DIG with the mode on
 *  the air, and the FT modes as DIG, whose mode the engine stamps from the tier it runs. A station
 *  too old to say logs as Phone, the voice logging the large log line exists for. */
export function contestLogMode(
  section: string | undefined,
  pskName: string | null,
): { fdMode: 'CW' | 'PH' | 'DIG'; fdSubmode?: string; mode: string; defaultRst: string } {
  switch (section) {
    case 'cw':
      return { fdMode: 'CW', mode: 'CW', defaultRst: '599' }
    case 'rtty':
      return { fdMode: 'DIG', fdSubmode: 'RTTY', mode: 'RTTY', defaultRst: '599' }
    case 'keyboard': {
      // The PSK cockpit's own fallback when its state has not arrived: PSK31.
      const name = pskName ?? PSK_MODE_BY_SLUG.psk31.name
      return { fdMode: 'DIG', fdSubmode: name, mode: name, defaultRst: '599' }
    }
    case 'digital':
      return { fdMode: 'DIG', mode: 'FT8', defaultRst: '599' }
    default:
      return { fdMode: 'PH', mode: 'SSB', defaultRst: '59' }
  }
}

/** The mode the station is on now, named as a contest's `bannedModes` names modes: on the digital
 *  section the FT tier it runs, on every other the mode its contacts log under. */
export function stationMode(section: string | undefined, tier: string | undefined, pskName: string | null): string {
  return section === 'digital' ? (tier ?? '') : contestLogMode(section, pskName).mode
}

/** The keyboard modes' sub-mode, which is the PSK cockpit's state rather than the snapshot's: read
 *  the way that cockpit reads it, while the station is on that section. */
export function usePskName(section: string | undefined): string | null {
  const [pskName, setPskName] = useState<string | null>(null)
  useEffect(() => {
    if (section !== 'keyboard') return
    let live = true
    getPskState()
      .then((s) => live && setPskName(PSK_MODE_BY_SLUG[s.mode ?? 'psk31']?.name ?? null))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [section])
  return pskName
}
