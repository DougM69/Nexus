// UI affordances follow the session's station authority. Enforcement remains in
// the transport and station; hiding/disabling a button never grants permission.
// The installed desktop keeps its existing authority and transmit guards.
import { createContext, useContext } from 'react'
import type { RadioStatus } from './types'

export const StationControlContext = createContext(true)
export function useStationControl(): boolean { return useContext(StationControlContext) }
export const StationDataContext = createContext(true)
export function useStationData(): boolean { return useContext(StationDataContext) }

/** STATION CONTROL HELD. The desktop holds its own station, so this is its local control. */
export function useStationHeld(): boolean {
  return useStationControl()
}

/** A permission for an explicit gesture. The desktop's own control carries every capability; the
 * native transmit guards still apply wherever the gesture lands. */
export function useStationCapability(_capability: string, _lapse = true): boolean {
  return useStationControl()
}

/** Local tier changes retain their native conditions. */
export function useStationTierControl(_radio: RadioStatus): boolean {
  return useStationControl()
}

/** Stop is independent of stale observations, receipts and ordinary commands. */
export function useStationStopControl(): boolean {
  return useStationControl()
}

/** How far a Stop has got: 'idle' on the desktop, where the call is the act. */
export function useStationStopProgress(_radio?: RadioStatus | null): 'idle' | 'sending' | 'sent' | 'stopped' {
  return 'idle'
}
