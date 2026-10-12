// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteFieldDay } from './RemoteFieldDay'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import type { ApplicationClient } from './application-client'
import { StationDataContext } from '../stationAccess'
import { installApplicationTransport } from '../applicationTransport'
import { fieldDayPage } from './__fixtures__/field-day-page'
import { parseFieldDay } from './field-day'
import { t } from '../i18n'

// THE BUDGET (2026-10-09). The slowest case here, "uses the real event boards and disclosures without…", takes 0.34 s
// and 0.37 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

afterEach(()=>{cleanup();vi.useRealTimers();vi.restoreAllMocks()})
function setup(invoke=vi.fn(async():Promise<unknown>=>fieldDayPage())) {
 const source=new RemoteCollections({invoke,supports:()=>true,getPhase:()=> 'ready'} as unknown as ApplicationClient)
 const view=(available=true)=><StationDataContext.Provider value={available}><RemoteCollectionsContext.Provider value={source}><RemoteFieldDay tier="FT8"/></RemoteCollectionsContext.Provider></StationDataContext.Provider>
 return {...render(view()),view,invoke}
}
it('uses the real event boards and disclosures without native loaders or station writes',async()=>{
 const forbidden=vi.fn(async()=>{throw new Error('unexpected native operation')})
 const uninstall=installApplicationTransport({kind:'remote',invoke:forbidden})
 try {
  const test=setup();await screen.findByText('K1ABC')
  expect(within(test.container.querySelector<HTMLElement>('[data-club-board]')!).getByText('CW tent')).toBeTruthy()
  expect(test.container.querySelector('.fd-score-math')?.textContent).toContain('106')
  const op=screen.getByRole('textbox',{name:t('fieldDay.operator.aria')}) as HTMLInputElement
  await waitFor(()=>expect(op.value).toBe('W1AW'));expect(op.readOnly).toBe(true)
  fireEvent.change(op,{target:{value:'K9NEW'}});fireEvent.blur(op)
  expect(test.container.querySelector('.fd-export')).toBeNull()
  const actions=[...test.container.querySelectorAll<HTMLButtonElement>('.fd-role-btn,.fd-power-chip,.fd-bonus-plan')]
  expect(actions).toHaveLength(21)
  expect(actions.every(e=>e.disabled)).toBe(true)
  const bonuses=[...test.container.querySelectorAll<HTMLInputElement>('.fd-bonus-row input')]
  expect(bonuses).toHaveLength(16)
  expect(bonuses.every(e=>e.disabled)).toBe(true)
  expect(test.container.querySelector('[data-bonus-state=planned]')?.textContent).toContain('Natural Power')
  fireEvent.click(test.container.querySelector('.fd-bonuses-toggle')!)
  fireEvent.click(screen.getByRole('button',{name:t('remote.fieldDayRefresh')}))
  await screen.findByText('K1ABC')
  expect(test.invoke).toHaveBeenCalledTimes(2)
  expect(test.container.querySelector('.fd-bonuses-list')).toBeNull()
  expect(forbidden).not.toHaveBeenCalled()
 } finally {uninstall()}
})
it('clears loss, refresh failures and inactive events without a fake zero score',async()=>{
 const test=setup();await screen.findByText('K1ABC')
 test.rerender(test.view(false));expect(test.container.textContent).not.toContain('K1ABC')
 test.rerender(test.view());await screen.findByText('K1ABC')
 test.invoke.mockRejectedValueOnce(new Error('applicationTooLarge'))
 fireEvent.click(screen.getByRole('button',{name:t('remote.fieldDayRefresh')}))
 await screen.findByText(t('remote.fieldDayTooLarge'))
 expect(test.container.textContent).not.toContain('K1ABC')
 const off=fieldDayPage();Object.assign((off.meta as {source:object}).source,{active:false,fieldDay:null})
 test.invoke.mockResolvedValueOnce(off)
 fireEvent.click(screen.getByRole('button',{name:t('remote.fieldDayRefresh')}))
 await screen.findByText(t('remote.fieldDayInactive'))
 expect(test.container.querySelector('.remote-field-day-bank:not([hidden])')).toBeNull()
})
// The page's own words are the contest's, whichever contest the station runs: it offered "Refresh Field Day" over an
// Illinois QSO Party, and said "No Field Day event is active" when no contest was running at all.
it('names the contest, not Field Day, on its refresh button and status lines',async()=>{
 const test=setup();await screen.findByText('K1ABC')
 expect(test.container.querySelector('.remote-insights-status span')?.textContent).toMatch(/^Station contest captured \d+ s ago\.$/)
 const off=fieldDayPage();Object.assign((off.meta as {source:object}).source,{active:false,fieldDay:null})
 test.invoke.mockResolvedValueOnce(off)
 fireEvent.click(screen.getByRole('button',{name:'Refresh contest'}))
 expect(await screen.findByText('No contest is running on the station.')).toBeTruthy()
})
it('ages club presence evidence and expires the whole captured event',async()=>{
 vi.useFakeTimers({toFake:['setInterval','clearInterval','performance']})
 const test=setup();await act(async()=>{await Promise.resolve()})
 expect(screen.getByText('K1ABC')).toBeTruthy()
 await act(async()=>{vi.advanceTimersByTime(7000)})
 expect(screen.getByTitle(t('fieldDay.club.board.stale',{secs:17}))).toBeTruthy()
 await act(async()=>{vi.advanceTimersByTime(60000)})
 expect(test.container.textContent).not.toContain('K1ABC')
 expect(test.container.textContent).not.toContain('CW tent')
})
// THE CLUB BOARD'S GRID ON THE HOSTED PAGE. The station leaves the host's link words out of what Remote reads
// (`leave_out_the_hosts_own`), so the page's grid places each position in the cell of its band and mode and colours none:
// no link class, no mark, no word, no legend. It is drawn from the capture the page is sent today, and the page still
// refuses a board line that carries a link, so the grid cannot bring one to it.
it('places each club position in the grid with no link colour, from the capture it is sent today',async()=>{
 const test=setup();await screen.findByText('K1ABC')
 const table=screen.getByRole('table',{name:t('fieldDay.club.grid.caption')})
 const [head,...rows]=within(table).getAllByRole('row')
 const cols=within(head).getAllByRole('columnheader').map(c=>c.textContent)
 const cw=rows.find(r=>within(r).getByRole('rowheader').textContent==='CW')!
 const cell=within(cw).getAllByRole('cell')[cols.indexOf('20m')-1]
 expect(cell.textContent).toBe('CW tent')
 const chip=cell.querySelector('.club-grid-pos')!
 expect([...chip.classList]).toEqual(['club-grid-pos'])
 expect(chip.querySelector('.club-grid-mark,.sr-only')).toBeNull()
 expect(chip.getAttribute('title')).toBeNull()
 expect(test.container.querySelector('.club-grid-legend')).toBeNull()
 // CONTROL: a board line carrying the host's link word is refused whole, so this page is never sent one.
 const page=fieldDayPage()
 const club=(page.meta as unknown as {source:{fieldDay:{club:{board:Record<string,unknown>[]}}}}).source.fieldDay.club
 expect(parseFieldDay(page).fieldDay?.club?.board).toHaveLength(1)
 club.board[0].link='connected'
 expect(()=>parseFieldDay(page)).toThrow('invalidFieldDay')
})
// Sweepstakes LOGS a duplicate and scores it zero, so the station counts one contact fewer than its log holds. The page
// refused that capture whole: its status line said the station was unavailable, and no contest view was drawn.
it('shows a contest log that holds a logged dupe, with the station\'s own count',async()=>{
 const page=fieldDayPage()
 const source=(page.meta as unknown as {source:{fieldDay:{event:string;log:Record<string,unknown>[]};ruleset:{event:string}}}).source
 source.fieldDay.event='arrlss_cw';source.ruleset.event='arrlss_cw'
 source.fieldDay.log.push({...source.fieldDay.log[1],whenUnix:1782583380,dupe:true})
 const test=setup(vi.fn(async()=>page))
 await waitFor(()=>expect(test.container.querySelector('.remote-insights-status span')?.textContent).not.toBe(t('remote.collectionLoading')))
 expect(test.container.querySelector('.remote-insights-status span')?.textContent).toBe(t('remote.fieldDaySnapshot',{seconds:0}))
 expect(test.container.querySelector('.fd-score-val')?.textContent).toBe('2')
 expect([...test.container.querySelectorAll('.fd-log-row')].map(row=>row.classList.contains('dupe')).sort()).toEqual([false,false,true])
})
it('keeps Winter Field Day raw-point scoring distinct from ARRL totals',async()=>{
 const page=fieldDayPage()
 const source=(page.meta as unknown as {source:{fieldDay:{event:string};ruleset:{event:string;bannedModes:string[]}}}).source
 source.fieldDay.event='wfd';source.ruleset.event='wfd';source.ruleset.bannedModes=['FT8']
 const test=setup(vi.fn(async()=>page));await screen.findByText('K1ABC')
 const math=test.container.querySelector('.fd-score-math')!.textContent!
 expect(math).toContain('3');expect(math).not.toContain('106');expect(math.toLowerCase()).not.toContain('power')
})
// The station's Contest screen takes contacts in its own window (its log line at the top, App.tsx); this page is an
// observation of that screen and logs nothing, so none of that log line reaches it.
it('shows no log line: no entry, no last contacts, no Call box',async()=>{
 const test=setup();await screen.findByText('K1ABC')
 expect(test.container.querySelector('.fd-event-banner'),'control: the contest screen is on the page').not.toBeNull()
 expect(test.container.querySelector('.contest-logger-entry,.contest-logger-recent,.le-fd-input-call,.log-entry-fd')).toBeNull()
})
