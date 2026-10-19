// @vitest-environment jsdom
//
// A CONTACT CLICKED WHERE ANOTHER VIEW LISTS IT OPENS IN THE LOGBOOK'S OWN EDITOR. The handoff
// (`focusCall` with an `editId`) names the contact by its id; the Logbook reads that row and its edit
// key from the engine and opens the form on it, and the save goes out through `editQsoById` with that
// id — the one write path. Asserted by value: the form holds the clicked contact, not a neighbour, and
// the save names its id.
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { waitOutTheListsScrollTimer } from './Logbook.testkit'
import { editQsoById } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { utcTime } from '../features/utcLog'
import { __resetLogSourceForTests } from '../features/logSource'
import type { LoggedQso } from '../types'

vi.setConfig({ testTimeout: 15_000 })

// `skew`: how many `locate` answers come back one place off, as when a contact is logged between the
// Logbook's two questions and the order moves under them.
const engine = vi.hoisted(() => ({ log: [] as unknown[], skew: 0 }))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: { kind: string }) => {
      const a = (await import('../features/logAnswers.testkit')).answerAs(q as never, engine.log as LoggedQso[], 1) as { index?: number | null }
      if (q.kind === 'locate' && engine.skew > 0 && a.index != null) {
        engine.skew--
        return { ...a, index: a.index + 1 }
      }
      return a
    }),
    deleteQsoById: noop(),
    editQsoById: vi.fn(async () => ({ kind: 'gone' })),
    exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(), logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(), markQslCardById: noop(), syncLotwReport: noop(), uploadLotwReport: noop(),
    qrzPushQso: noop(), clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((run: () => Promise<unknown>) => run()) }))

const qso = (id: string, call: string, i: number) =>
  ({
    id, call, grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'SSB', rstSent: '59', rstRcvd: '57',
    name: `Name ${call}`, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000 + i * 60,
    confirmed: false, awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null,
  }) as unknown as LoggedQso

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
beforeEach(() => {
  __resetLogSourceForTests()
  // Two contacts with the SAME call: the search the handoff sets shows both, so only the id can
  // say which one the form opened on.
  engine.skew = 0
  engine.log = [qso('id-a', 'W1AW', 0), qso('id-b', 'K1ABC', 1), qso('id-c', 'W1AW', 2), qso('id-d', 'N2XYZ', 3)]
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})
afterAll(waitOutTheListsScrollTimer)

const props = { defaultBand: '20m', defaultFreqMhz: 14.2, defaultMode: 'SSB' }

it('a handoff with an id opens the editor on exactly that contact, and the save names its id', async () => {
  render(<Logbook {...props} focusCall={{ call: 'W1AW', ts: 1, editId: 'id-a' }} />)
  const call = (await screen.findByLabelText(t('logbook.field.call.label'))) as HTMLInputElement
  expect(call.value).toBe('W1AW')
  // id-a is the OLDER W1AW: its time is what the form holds, not id-c's (two minutes later).
  expect(screen.getByDisplayValue(utcTime(1_700_000_000))).toBeTruthy()
  expect(screen.queryByDisplayValue(utcTime(1_700_000_120))).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
  await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(1))
  const [ref] = vi.mocked(editQsoById).mock.calls[0]
  expect(ref.id).toBe('id-a')
})

it('the other W1AW opens on its own id', async () => {
  render(<Logbook {...props} focusCall={{ call: 'W1AW', ts: 2, editId: 'id-c' }} />)
  await screen.findByLabelText(t('logbook.field.call.label'))
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
  await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(1))
  expect(vi.mocked(editQsoById).mock.calls[0][0].id).toBe('id-c')
})

it('a handoff naming a contact no longer in the log opens no form and says so', async () => {
  render(<Logbook {...props} focusCall={{ call: 'W1AW', ts: 3, editId: 'id-gone' }} />)
  await waitFor(() => expect(pushToast).toHaveBeenCalledWith(t('logbook.form.editGone', { call: 'W1AW' }), 'error'))
  expect(screen.queryByRole('button', { name: t('logbook.form.save') })).toBeNull()
})

it('a handoff without an id still only searches (the recall card on a pre-1.14 station)', async () => {
  render(<Logbook {...props} focusCall={{ call: 'W1AW', ts: 4 }} />)
  await screen.findByDisplayValue('W1AW')
  expect(screen.queryByRole('button', { name: t('logbook.form.save') })).toBeNull()
})

it('an order that moved between the two questions is asked again, never edited at the wrong row', async () => {
  engine.skew = 1
  render(<Logbook {...props} focusCall={{ call: 'K1ABC', ts: 5, editId: 'id-b' }} />)
  const call = (await screen.findByLabelText(t('logbook.field.call.label'))) as HTMLInputElement
  expect(call.value, 'the form opened on the row the stale place named').toBe('K1ABC')
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
  await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(1))
  expect(vi.mocked(editQsoById).mock.calls[0][0].id).toBe('id-b')
})
