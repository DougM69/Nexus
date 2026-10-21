// SPOTS FROM PROGRAMS ON NEXUS'S TCI SERVER ("Share by TCI"), for the band map and the band strip.
//
// A program connected to the station's TCI server (a logger such as Log4OM, a skimmer) sends its
// spots, and the station keeps them apart from the cluster's, bounded and aged. Each comes here as a
// spot row whose `tci` names the program that sent it, so the band map draws it as it draws a
// cluster spot, with its source beside the call.
//
// ⛔ A CLICK ON ONE ONLY TUNES. The band map hands a program's spot to `onTciSpot`, never to the
// Work a cluster spot gets: the station tunes there and tells every program of the click
// (`clickTciSpot`), and it never changes the section, arms transmit or starts a QSO.
//
// The station says when they changed (`radio.tciServer.spots`, a generation in the snapshot), so a
// window asks for them only then, and again every 15 s while some are on the map, so their ages
// stay true. None while the server is off, has none, or the station is older than this window.
import { useEffect, useState } from 'react'
import { getTciSpots } from '../api'
import type { SpotRow } from '../types'

/** While programs' spots are on the map, how often they are asked again so their ages stay true. */
export const TCI_SPOTS_REFRESH_MS = 15_000

/** The spots programs sent, for `generation` (`radio.tciServer.spots`): none for 0 or none at all. */
export function useTciSpots(generation: number | null | undefined): SpotRow[] {
  const [spots, setSpots] = useState<SpotRow[]>([])
  useEffect(() => {
    if (!generation) {
      setSpots([])
      return
    }
    let live = true
    let again: ReturnType<typeof setTimeout> | undefined
    const load = () => {
      getTciSpots()
        .then((rows) => {
          if (!live) return
          // Only a program's spot: a row naming none is not one, and must never be clicked as one.
          const ours = Array.isArray(rows) ? rows.filter((r) => r.tci != null) : []
          setSpots(ours)
          if (ours.length > 0) again = setTimeout(load, TCI_SPOTS_REFRESH_MS)
        })
        .catch(() => {})
    }
    load()
    return () => {
      live = false
      if (again !== undefined) clearTimeout(again)
    }
  }, [generation])
  return spots
}
