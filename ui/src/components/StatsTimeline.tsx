// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT: the mode names the lines are named by, the UTC
// times and dates the buckets are named by, and the counts (all data).
//
// The Statistics view's Timeline card: QSOs per mode over the past day (by hour), the past 30 days
// (by day) or the past year (by month), each window ending with the current UTC hour, day or month
// — UTC, as the log is (tempo-core `TimelineRange` holds the rule). The engine counts where the log
// is (`getLogTimeline`), one range at a time; this draws its answer by hand in SVG, as the other
// cards are CSS bars — no chart library.
//
// A line is a colour AND a dash pattern AND a marker shape (styles.css, `.stats-tl`), so two lines
// stay apart without colour. Pointing at the chart, or focusing it and stepping with the arrow
// keys, shows one bucket's counts under it; a table lists every count for a screen reader.

import { useEffect, useId, useMemo, useState, type KeyboardEvent, type PointerEvent } from 'react'
import { getLogTimeline } from '../api'
import { t } from '../i18n'
import type { LogTimeline, TimelineRange } from '../types'
import { TICK_EVERY, bucketName, tickName, timelineLines, type TimelineLine } from '../features/logTimeline'
import { askAgainWhileSaving } from '../features/notAnswered'

const RANGES: readonly TimelineRange[] = ['pastDay', 'past30Days', 'pastYear']

/** A range's name, on its switch. */
function rangeName(range: TimelineRange): string {
  if (range === 'pastDay') return t('stats.timeline.range.pastDay')
  return range === 'past30Days' ? t('stats.timeline.range.past30Days') : t('stats.timeline.range.pastYear')
}

/** What the table calls a range's buckets. */
function bucketColumn(range: TimelineRange): string {
  if (range === 'pastDay') return t('stats.timeline.col.pastDay')
  return range === 'past30Days' ? t('stats.timeline.col.past30Days') : t('stats.timeline.col.pastYear')
}

/** The range the card opens on: thirty days is the one most likely to hold contacts — a day often
 * holds none — while still showing the operator's recent days one by one. */
const FIRST_RANGE: TimelineRange = 'past30Days'

/** A line's name: its mode, or "Other". */
const lineName = (l: TimelineLine) => l.mode ?? t('stats.timeline.other')

/** The attributes that give a line its slot's colour, dash and marker (styles.css). */
const slotOf = (l: TimelineLine, slot: number) => ({
  'data-slot': slot,
  'data-other': l.mode === null || undefined,
})

export function StatsTimeline() {
  const [range, setRange] = useState<TimelineRange>(FIRST_RANGE)
  // The last answer, with the range it answers; `null` for a range whose read failed.
  const [answer, setAnswer] = useState<{ range: TimelineRange; timeline: LogTimeline | null } | null>(null)
  useEffect(() => {
    let live = true
    // Asked again while a contact logged just before is still being saved (features/notAnswered).
    void askAgainWhileSaving(() => getLogTimeline(range), () => live)
      .then((timeline) => { if (live) setAnswer({ range, timeline }) })
      .catch(() => { if (live) setAnswer({ range, timeline: null }) })
    return () => { live = false }
  }, [range])
  const shown = answer?.range === range ? answer : null
  const lines = useMemo(() => (shown?.timeline ? timelineLines(shown.timeline) : []), [shown])

  return (
    <div className="stats-card stats-tl">
      <div className="stats-tl-head">
        <h3>{t('stats.timeline.head')}</h3>
        <div className="stats-tl-ranges" role="group" aria-label={t('stats.timeline.range.aria')}>
          {RANGES.map((r) => (
            <button
              key={r}
              type="button"
              className={`filter-chip${r === range ? ' active' : ''}`}
              aria-pressed={r === range}
              onClick={() => setRange(r)}
            >
              {rangeName(r)}
            </button>
          ))}
        </div>
      </div>
      {!shown ? (
        <p className="stats-empty">{t('stats.loading')}</p>
      ) : !shown.timeline ? (
        <p className="stats-empty">{t('stats.failed')}</p>
      ) : lines.length === 0 ? (
        <p className="stats-empty">{t('stats.timeline.empty')}</p>
      ) : (
        <TimelineChart range={range} timeline={shown.timeline} lines={lines} />
      )}
    </div>
  )
}

function TimelineChart({ range, timeline, lines }: { range: TimelineRange; timeline: LogTimeline; lines: TimelineLine[] }) {
  // `useId` has colons; an SVG reference reads cleaner without them.
  const id = `stl${useId().replace(/:/g, '')}`
  const n = timeline.edges.length - 1
  // The picked bucket: pointed at, or stepped to from the keyboard.
  const [at, setAt] = useState<number | null>(null)
  const top = Math.max(1, ...lines.flatMap((l) => l.counts))
  // Each point at its bucket's middle, as a percentage of the plot.
  const x = (i: number) => ((i + 0.5) / n) * 100
  const y = (count: number) => 100 - (count / top) * 100
  const ticks = [...Array(n).keys()].filter((i) => (n - 1 - i) % TICK_EVERY[range] === 0)
  const pick = (i: number) => setAt(Math.min(n - 1, Math.max(0, i)))

  const pointAt = (e: PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect()
    if (box.width > 0) pick(Math.floor(((e.clientX - box.left) / box.width) * n))
  }
  const step = (e: KeyboardEvent<HTMLDivElement>) => {
    const moves: Partial<Record<string, number>> = {
      ArrowLeft: (at ?? n) - 1,
      ArrowRight: (at ?? n - 2) + 1,
      Home: 0,
      End: n - 1,
    }
    const to = moves[e.key]
    if (to === undefined) return
    e.preventDefault()
    pick(to)
  }

  return (
    <>
      <div className="stats-tl-chart">
        <div className="stats-tl-y mono" aria-hidden="true">
          <span>{top}</span>
          <span>0</span>
        </div>
        <div
          className="stats-tl-plot"
          tabIndex={0}
          role="group"
          aria-label={t('stats.timeline.chart', { range: rangeName(range) })}
          aria-describedby={`${id}-read`}
          onPointerMove={pointAt}
          onPointerLeave={() => setAt(null)}
          onFocus={() => setAt((a) => a ?? n - 1)}
          onBlur={() => setAt(null)}
          onKeyDown={step}
        >
          {/* The lines, stretched to the plot; their strokes and dashes stay in screen pixels. */}
          <svg className="stats-tl-lines" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
            {lines.map((l, slot) => (
              <polyline
                key={l.mode ?? ''}
                className="stats-tl-line"
                {...slotOf(l, slot)}
                points={l.counts.map((c, i) => `${x(i).toFixed(3)},${y(c).toFixed(3)}`).join(' ')}
                vectorEffect="non-scaling-stroke"
              />
            ))}
          </svg>
          {/* The markers, placed by percentage in an unscaled box so each keeps its shape. */}
          <svg className="stats-tl-marks" aria-hidden="true">
            <defs>
              <circle id={`${id}-0`} r="3.5" />
              <rect id={`${id}-1`} x="-3" y="-3" width="6" height="6" />
              <path id={`${id}-2`} d="M0 -4.2 L3.9 3 H-3.9 Z" />
              <path id={`${id}-3`} d="M0 -4.4 L4.4 0 L0 4.4 L-4.4 0 Z" />
              <path id={`${id}-4`} d="M-3.2 -3.2 L3.2 3.2 M3.2 -3.2 L-3.2 3.2" />
              <path id={`${id}-5`} d="M-4 0 H4 M0 -4 V4" />
            </defs>
            {at !== null && <line className="stats-tl-guide" x1={`${x(at)}%`} x2={`${x(at)}%`} y1="0" y2="100%" />}
            {lines.map((l, slot) =>
              l.counts.map((c, i) => (
                <use
                  key={`${l.mode ?? ''}:${i}`}
                  className="stats-tl-mark"
                  {...slotOf(l, slot)}
                  href={`#${id}-${slot}`}
                  x={`${x(i)}%`}
                  y={`${y(c)}%`}
                />
              )),
            )}
          </svg>
        </div>
        <div className="stats-tl-x mono" aria-hidden="true">
          {ticks.map((i) => (
            <span key={i} className="stats-tl-tick" style={{ left: `${x(i)}%` }}>
              {tickName(range, timeline.edges[i])}
            </span>
          ))}
        </div>
      </div>

      <ul className="stats-tl-legend">
        {lines.map((l, slot) => (
          <li key={l.mode ?? ''} {...slotOf(l, slot)}>
            <svg className="stats-tl-key" viewBox="-12 -5 24 10" aria-hidden="true">
              <line className="stats-tl-line" x1="-12" x2="12" y1="0" y2="0" />
              <use className="stats-tl-mark" href={`#${id}-${slot}`} />
            </svg>
            <span className="stats-tl-name">{lineName(l)}</span>
            <span className="stats-tl-total mono">{l.total}</span>
          </li>
        ))}
      </ul>

      <p className="stats-tl-read" id={`${id}-read`} aria-live="polite">
        {at === null ? (
          <span className="stats-tl-hint">{t('stats.timeline.hint')}</span>
        ) : (
          <>
            <span className="stats-tl-when mono">{bucketName(range, timeline.edges[at])}</span>
            {lines.map((l, slot) => (
              <span key={l.mode ?? ''} className="stats-tl-at" {...slotOf(l, slot)}>
                <span className="stats-tl-name">{lineName(l)}</span>
                <span className="mono">{l.counts[at]}</span>
              </span>
            ))}
          </>
        )}
      </p>

      <table className="sr-only">
        <caption>{t('stats.timeline.table', { range: rangeName(range) })}</caption>
        <thead>
          <tr>
            <th scope="col">{bucketColumn(range)}</th>
            {lines.map((l) => (
              <th scope="col" key={l.mode ?? ''}>
                {lineName(l)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {timeline.edges.slice(0, n).map((start, i) => (
            <tr key={start}>
              <th scope="row">{bucketName(range, start)}</th>
              {lines.map((l) => (
                <td key={l.mode ?? ''}>{l.counts[i]}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </>
  )
}
