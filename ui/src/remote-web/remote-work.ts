// Which needs and spots a browser may Work. CW and Phone ride the station's workSpot intent;
// FT8/FT4 ride workDigitalSpot, which names its tier; RTTY rides workRttySpot. Work tunes the
// station and sets its mode and tier; it never starts a QSO or enables transmit.
import { workTarget, workTier } from '../features/needs'
import type { BandChannel, NeedAlert, SpotRow } from '../types'

/** The station hints a browser Work needs, plus the cockpit switches the desktop Work obeys. */
export type RemoteWorkGrants = { workSpot: boolean; workDigitalSpot: boolean; workRttySpot: boolean
  cwEnabled: boolean; phoneEnabled: boolean; rttyEnabled: boolean }

/** The FT8/FT4 tier a need names, or null when it names none: its own mode, as the desktop's Work
 * reads it (`workTier`), so a POTA row heard in FT4 works on FT4 from a browser too. */
export function remoteWorkTier(alert: Pick<NeedAlert, 'mode' | 'exactMode'>): 'FT8' | 'FT4' | null {
  return workTier(alert)
}

export function remoteWorkable(alert: NeedAlert, bandPlan: BandChannel[], grants: RemoteWorkGrants): boolean {
  const target = workTarget(alert, bandPlan)
  if (!target) return false
  // Any other digital need (JS8, FT2, a band-level "Digital" row) has no remote transaction.
  if (target.view === 'operate') return grants.workDigitalSpot && remoteWorkTier(alert) !== null
  // RTTY is its own station intent and its own cockpit switch: the desktop Work QSYs instead of
  // navigating when the cockpit is off, and a browser has no cockpit to land in at all.
  if (target.view === 'rtty') return grants.workRttySpot && grants.rttyEnabled
  return grants.workSpot && ((target.view === 'cw' && grants.cwEnabled) || (target.view === 'phone' && grants.phoneEnabled))
}

/** A Spots row in the Needed-board shape the Work path takes. FT8/FT4 keep their submode so the
 * tier travels with the spot. */
export function spotNeed(s: SpotRow): NeedAlert {
  return {
    call: s.call,
    entity: s.entity,
    band: s.band,
    zone: s.zone,
    tags: [],
    priority: 0,
    headline: '',
    mode: s.submode === 'FT4' || s.submode === 'FT8' ? s.submode : s.mode,
    freqMhz: s.freqMhz,
  }
}
