// A held message too long to hand over at the current speed. Its hand-over (QUERY MSG n's answer,
// "<call> MSG <text> FROM <sender>") needs more frames than one transmission holds at this speed
// (frames × period under ten minutes, §97.119), so the station neither offers it (QUERY MSGS, a
// heartbeat's acknowledgement, NEXT MSG ID) nor hands it over until the speed allows it. JS8Call
// has no such cap. Store… refuses such a message up front; one that came over the air says so on
// its own Inbox row, with the speed it goes at (`Station::held_too_long`).
import type { Js8InboxTooLong } from '../types'
import { t } from '../i18n'
import { JS8_SPEEDS } from '../js8Vocab'

export function Js8InboxTooLongNote({
  id,
  tooLong,
}: {
  /** The Inbox row's id. */
  id: number
  /** `Js8State.inboxTooLong`: absent while there are none. */
  tooLong: Js8InboxTooLong[] | undefined
}) {
  const note = tooLong?.find((n) => n.id === id)
  if (!note) return null
  return (
    <span className="js8-cell js8-path">
      {note.fitsAt
        ? t('js8.inbox.tooLong.fits', {
            frames: note.frames,
            max: note.max,
            speed: JS8_SPEEDS[note.fitsAt].label,
          })
        : t('js8.inbox.tooLong.never', { frames: note.frames })}
    </span>
  )
}
