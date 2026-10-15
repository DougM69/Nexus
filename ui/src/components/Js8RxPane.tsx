// THE RX PANE, as JS8Call's (its top text box, `textEditRX`): the traffic on my offset and what is
// addressed to me, each message on one line as its frames arrive — "time (offset) text" — closed by
// the end mark, my own transmissions among them (features/js8Lists `js8RxLines`). Every frame of
// every offset stays in Band activity.
//
// A display: nothing here keys. A double-click on a line that opens with a callsign puts that call
// in the To box, as before.
//
// ⚠️ Migrated from birth (i18n/hardcoded-strings.test.ts): the times, offsets and the received text
// are data; every word is in the catalog.
import type { Ref, UIEventHandler } from 'react'
import { t } from '../i18n'
import { utcClock } from '../js8Vocab'
import type { Js8RxLine } from '../features/js8Lists'

export function Js8RxPane({
  lines,
  scrollRef,
  onScroll,
  onSelectCall,
}: {
  lines: readonly Js8RxLine[]
  /** The feed's follow-the-bottom scroller (usePinnedScroll). */
  scrollRef: Ref<HTMLDivElement>
  onScroll: UIEventHandler<HTMLDivElement>
  /** A double-click on a line opening with a call. */
  onSelectCall: (call: string) => void
}) {
  return (
    <div className="js8-activity js8-rx" ref={scrollRef} onScroll={onScroll} title={t('js8.rx.title')}>
      {lines.length === 0 ? (
        <div className="cw-decode-idle">{t('js8.rx.empty')}</div>
      ) : (
        lines.map((l) => (
          <div
            key={l.key}
            className={`js8-row js8-rx-line${l.mine ? ' mine' : ''}${l.directed ? ' directed' : ''}`}
            onDoubleClick={l.from ? () => onSelectCall(l.from) : undefined}
            title={l.from ? t('js8.rx.line.title', { call: l.from }) : undefined}
          >
            <span className="js8-cell js8-time">{utcClock(l.atMs)}</span>
            <span className="js8-cell js8-freq">{l.offsetHz}</span>
            <span className="js8-cell js8-text">{l.text}</span>
          </div>
        ))
      )}
    </div>
  )
}
