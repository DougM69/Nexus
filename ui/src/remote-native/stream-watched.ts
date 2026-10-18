// Whether a stream watches this station's screen. The station tells its main window each change as
// the Tauri event `remote-stream-watched`; while it is `true`, the screen shows no stored key and the
// station sends it none (`get_settings`).
import { useEffect, useState } from 'react'
import { getStreamWatched } from '../api'

export const STREAM_WATCHED_EVENT = 'remote-stream-watched'

/** Whether a stream watches this screen: asked once, then told each change. `false` without the
 *  event bridge (a plain browser preview) or where the station does not answer. */
export function useStreamWatched(): boolean {
  const [watched, setWatched] = useState(false)
  useEffect(() => {
    let alive = true
    let told = false
    let unlisten: (() => void) | undefined
    const listen = window.__TAURI__?.event?.listen
    if (listen) {
      void listen<boolean>(STREAM_WATCHED_EVENT, (e) => {
        told = true
        if (alive) setWatched(e.payload === true)
      }).then(
        (un) => { if (alive) unlisten = un; else un() },
        () => { /* a listener that fails to register must not take the app down */ },
      )
    }
    // The answer to the first ask is older than any change told since.
    getStreamWatched().then(
      (live) => { if (alive && !told) setWatched(live === true) },
      () => {},
    )
    return () => { alive = false; unlisten?.() }
  }, [])
  return watched
}
