// PER-OPERATOR CONTEST STATISTICS — the Contest summary's Operators section and the Operators box.
//
// One table component for both, so the summary and the box cannot disagree about a figure: every
// number is the station's (`FieldDayStatus.operators`, scored in tempo-core by the contest's own
// rules), and the shares are features/contestOperators'. Callsigns, bands, mode classes and counts are
// invariant tokens: only the headings and the notes are translated.
import { t } from '../i18n'
import { T } from '../i18n/T'
import type { FdOperatorStat, FieldDayStatus } from '../types'
import {
  UNNAMED_OPERATOR,
  bandModeCells,
  bandRank,
  operatorLabel,
  operatorRows,
  showsOperatorSection,
  utcHourLabel,
} from '../features/contestOperators'

function Share({ n, pct }: { n: number; pct: number | null }) {
  return (
    <>
      {n}
      {pct !== null && <span className="fd-ops-share">{` ${pct}%`}</span>}
    </>
  )
}

function OperatorName({ op }: { op: string }) {
  return op === '' ? (
    <span title={t('fieldDay.operators.unnamed.title')}>{UNNAMED_OPERATOR}</span>
  ) : (
    <>{op}</>
  )
}

/**
 * The leaderboard, best first. `compact` is the box's: operator, contacts, points and the last hour's
 * rate. The full table (the summary's) adds each operator's new multipliers when the contest counts
 * multipliers, the last 10 minutes' rate and the best clock hour.
 */
export function OperatorTable({
  ops,
  withMults,
  compact = false,
}: {
  ops: readonly FdOperatorStat[]
  withMults: boolean
  compact?: boolean
}) {
  const rows = operatorRows(ops)
  return (
    <table className="fd-ops-table">
      <caption className="sr-only">{t('fieldDay.operators.caption')}</caption>
      <thead>
        <tr>
          <th scope="col">{t('fieldDay.operators.col.operator')}</th>
          <th scope="col">{t('fieldDay.operators.col.qsos')}</th>
          <th scope="col">{t('fieldDay.operators.col.points')}</th>
          {!compact && withMults && <th scope="col">{t('fieldDay.operators.col.mults')}</th>}
          {!compact && <th scope="col">{t('fieldDay.operators.col.rate10')}</th>}
          {compact ? (
            <th scope="col" title={t('fieldDay.operators.col.rate60')}>
              {t('fieldDay.operators.col.rate60.short')}
            </th>
          ) : (
            <th scope="col">{t('fieldDay.operators.col.rate60')}</th>
          )}
          {!compact && <th scope="col">{t('fieldDay.operators.col.best')}</th>}
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.operator} data-operator={operatorLabel(r.operator)}>
            <th scope="row">
              <OperatorName op={r.operator} />
            </th>
            <td>{compact ? r.qsos : <Share n={r.qsos} pct={r.qsoShare} />}</td>
            <td>{compact ? r.points : <Share n={r.points} pct={r.pointShare} />}</td>
            {!compact && withMults && (
              <td>
                <Share n={r.newMults} pct={r.multShare} />
              </td>
            )}
            {!compact && <td>{r.rate10}</td>}
            <td>{r.rate60}</td>
            {!compact && <td>{r.bestHourQsos > 0 ? `${utcHourLabel(r.bestHourStartUnix)} ${r.bestHourQsos}` : '—'}</td>}
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/** One operator's contacts by band (rows) and mode class (columns). */
function BandModeGrid({ op }: { op: FdOperatorStat }) {
  const cells = bandModeCells(op)
  const bands = [...new Set(cells.map((c) => c.band))].sort((a, b) => bandRank(a) - bandRank(b) || a.localeCompare(b))
  const modes = [...new Set(cells.map((c) => c.mode))].sort()
  const n = (band: string, mode: string) => cells.find((c) => c.band === band && c.mode === mode)?.qsos ?? 0
  return (
    <table className="fd-ops-grid" data-operator={operatorLabel(op.operator)}>
      <caption>
        <OperatorName op={op.operator} />
      </caption>
      <thead>
        <tr>
          <th scope="col">{t('fieldDay.operators.col.band')}</th>
          {modes.map((m) => (
            <th scope="col" key={m}>
              {m}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {bands.map((b) => (
          <tr key={b}>
            <th scope="row">{b}</th>
            {modes.map((m) => (
              <td key={m}>{n(b, m) || ''}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/**
 * ⭐ THE SUMMARY'S OPERATORS SECTION — under the multiplier boards. Drawn only with two or more
 * operators (features/contestOperators `showsOperatorSection`): a single operator's row would repeat
 * the score tiles above it.
 */
export function OperatorStatsBoard({ fieldDay }: { fieldDay: FieldDayStatus | null }) {
  const ops = fieldDay?.operators
  if (!ops || !showsOperatorSection(ops)) return null
  return (
    <section className="fd-ops" aria-label={t('fieldDay.operators.heading')}>
      <h3 className="fd-ops-heading">{t('fieldDay.operators.heading')}</h3>
      <div className="fd-ops-scroll">
        <OperatorTable ops={ops} withMults={fieldDay.multCount != null} />
      </div>
      <div className="fd-ops-grids">
        {ops.map((o) => (
          <BandModeGrid key={o.operator} op={o} />
        ))}
      </div>
      <p className="fd-ops-note">{t('fieldDay.operators.note')}</p>
    </section>
  )
}

/**
 * ⭐ THE OPERATORS BOX — who is at the key now and the leaderboard, beside a cockpit or on Conditions.
 * With no contest running the box's own one line says so (its `basic`), so this renders only with a
 * contest; a single operator is one row, because here the row says who is on the board at all.
 */
export function OperatorsBox({ fieldDay }: { fieldDay: FieldDayStatus }) {
  const ops = fieldDay.operators ?? []
  const now = fieldDay.operatorNow ?? ''
  return (
    <div className="fd-ops fd-ops-box">
      <p className="fd-ops-atkey">
        {now ? (
          <T k="fieldDay.operators.atKey" vals={{ call: now }} tags={{ b: <strong /> }} />
        ) : (
          t('fieldDay.operators.nobody')
        )}
      </p>
      {ops.length > 0 ? (
        <OperatorTable ops={ops} withMults={false} compact />
      ) : (
        <p className="cp-none">{t('fieldDay.operators.none')}</p>
      )}
    </div>
  )
}
