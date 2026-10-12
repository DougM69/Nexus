// @vitest-environment jsdom
//
// The warn-only FD advisories, driven by FIXTURE DTOs (the component is pure by
// design — see FdAdvisories.tsx). Two properties matter enough to pin:
//
//   WARN, NEVER REMOVE — the advisory is a passive status line. Nothing here
//   renders a control, a `disabled`, or a panel-vocabulary id.
//
//   DORMANT = INVISIBLE — the 2026 seed ships assistance fully allowed, so the
//   assistance advisory renders NOWHERE today. That invisibility is pinned WITH
//   a restricted-fixture positive control right beside it, so the dormancy test
//   cannot pass vacuously (a component that never renders anything would fail
//   the control).
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { FdAdvisories, NoCreditChip } from './FdAdvisories'
import type { FdRulesetDto } from '../api'
import { contestName } from '../fdEvent'
import { t } from '../i18n'

// The shipped 2026 WFD ruleset facts, as get_fd_ruleset serves them (dormant
// assistance; the WSJT-suite ban).
const WFD: FdRulesetDto = {
  event: 'wfd',
  rulesYear: 2026,
  bannedModes: ['FST4', 'FT4', 'FT8', 'JT4', 'JT9', 'JT65', 'Q65', 'MSK144', 'WSPR', 'FST4W', 'ECHO'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
}

// ARRL FD bans nothing.
const SFD: FdRulesetDto = {
  event: 'arrlfd',
  rulesYear: 2026,
  bannedModes: [],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
}

const advisories = () => [...document.querySelectorAll('.fd-advisory')]

afterEach(cleanup)

describe('the banned-mode chip (the first consumer bannedModes has ever had)', () => {
  it('renders for a banned tier at WFD, citing event + rules year', () => {
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT8" />)
    const chip = document.querySelector('.fd-advisory.banned')
    expect(chip, 'FT8 is banned at WFD — the chip must render').not.toBeNull()
    expect(chip!.textContent).toContain('FT8')
    expect(chip!.textContent).toContain('Winter Field Day')
    expect(chip!.textContent).toContain('2026')
  })

  it('is absent for a legal tier at WFD (FT2 is not a WSJT mode)', () => {
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT2" />)
    expect(advisories()).toHaveLength(0)
  })

  it('is absent at ARRL FD (bans nothing), and absent with the master switch off', () => {
    render(<FdAdvisories fdActive ruleset={SFD} activeMode="FT8" />)
    expect(advisories()).toHaveLength(0)
    cleanup()
    render(<FdAdvisories fdActive={false} ruleset={WFD} activeMode="FT8" />)
    expect(advisories()).toHaveLength(0)
  })

  it('warns only — no control, no disabled, anywhere', () => {
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT8" />)
    expect(advisories().length).toBeGreaterThan(0)
    expect(document.querySelector('button, input, select, a, [disabled]')).toBeNull()
  })
})

// ⭐ THE LARGE LOG LINE'S CHIP (2026-10-12): the banned-mode line's fact in two words, for the one
// surface the operator ruled on ("A small outlined chip beside CONTEST LOG / PH that reads 'FT8 · no
// credit'; hover it for the full sentence. No yellow band."). The banner and the cockpit header keep
// the line. Pinned: the chip shows for exactly the inputs the line shows for, carrying its sentence.
describe('the no-credit chip', () => {
  const sentence = (mode: string, event: string) => t('fieldDay.advisory.banned', { mode, event: contestName(event), year: 2026 })
  const ILQP: FdRulesetDto = { ...SFD, event: 'ilqp', bannedModes: ['FT8', 'FT4'] }

  it('reads "<mode> · no credit", with the whole sentence as its title and its accessible description', () => {
    render(<NoCreditChip fdActive ruleset={ILQP} activeMode="FT4" />)
    const chip = screen.getByRole('status', { name: 'FT4 · no credit', description: sentence('FT4', 'ilqp') })
    expect(chip.textContent).toBe('FT4 · no credit')
    expect(chip.getAttribute('title')).toBe(sentence('FT4', 'ilqp'))
  })

  it('shows for exactly the inputs the banned line shows for, with that line’s sentence', () => {
    const rulesets = [WFD, SFD, ILQP, null, undefined]
    const modes = ['FT8', 'ft8', ' Ft4 ', 'FT2', 'SSB', 'CW', '', undefined]
    let shown = 0
    let cases = 0
    for (const fdActive of [true, false])
      for (const ruleset of rulesets)
        for (const activeMode of modes) {
          const what = `fdActive=${fdActive} ${ruleset?.event ?? ruleset} ${JSON.stringify(activeMode)}`
          const line = render(<FdAdvisories fdActive={fdActive} ruleset={ruleset} activeMode={activeMode} />)
          const said = line.container.querySelector('.fd-advisory.banned')?.getAttribute('title') ?? null
          line.unmount()
          const chip = render(<NoCreditChip fdActive={fdActive} ruleset={ruleset} activeMode={activeMode} />)
          const el = chip.container.firstElementChild
          expect(el?.getAttribute('title') ?? null, what).toBe(said)
          if (said !== null) expect(el!.textContent, what).toBe(`${activeMode!.trim().toUpperCase()} · no credit`)
          chip.unmount()
          cases++
          if (said !== null) shown++
        }
    // Neither side can pass by never rendering, or by always rendering: FT8, ft8 and Ft4 at the
    // Illinois QSO Party, and FT8, ft8 and Ft4 at Winter Field Day.
    expect([shown, cases]).toEqual([6, 80])
  })

  it('warns only — no control, no disabled', () => {
    render(<NoCreditChip fdActive ruleset={ILQP} activeMode="FT8" />)
    expect(document.body.textContent).toBe('FT8 · no credit')
    expect(document.querySelector('button, input, select, a, [disabled], [tabindex]')).toBeNull()
  })
})

// ⭐ THE EVENT NAMED IS THE CONTEST THAT IS RUNNING. The name was read as "Winter Field Day"
// for `wfd` and "ARRL Field Day" for LITERALLY EVERYTHING ELSE, so a contest that bans a mode
// would have told its operator the ban came from Field Day. Latent until a ruleset other than
// WFD banned something — the Illinois QSO Party bans FT8 and FT4.
describe('the event the advisory names', () => {
  it('is the running contest, not Field Day', () => {
    const rtty: FdRulesetDto = { ...WFD, event: 'cqww_rtty' }
    render(<FdAdvisories fdActive ruleset={rtty} activeMode="FT8" />)
    const chip = document.querySelector('.fd-advisory.banned')!
    expect(chip.textContent).toContain('CQ World-Wide RTTY DX Contest')
    expect(chip.textContent).not.toContain('Field Day')
    // A ruleset id this build's picker does not list is shown as itself — a token, never
    // another contest's name.
    cleanup()
    render(<FdAdvisories fdActive ruleset={{ ...WFD, event: 'zzqp' }} activeMode="FT8" />)
    expect(document.querySelector('.fd-advisory.banned')!.textContent).toContain('zzqp')
    // POSITIVE CONTROLS: both Field Day events still name themselves.
    cleanup()
    render(<FdAdvisories fdActive ruleset={WFD} activeMode="FT8" />)
    expect(document.querySelector('.fd-advisory.banned')!.textContent).toContain('Winter Field Day')
    cleanup()
    render(<FdAdvisories fdActive ruleset={{ ...SFD, bannedModes: ['FT8'] }} activeMode="FT8" />)
    expect(document.querySelector('.fd-advisory.banned')!.textContent).toContain('ARRL Field Day')
  })
})

describe('the assistance advisory', () => {
  const LIVE = ['AI CW decoder', 'DX cluster / RBN', 'PSK Reporter needs']

  it('DORMANT (the shipped 2026 seed): everything allowed ⇒ invisible even with every source live', () => {
    render(<FdAdvisories fdActive ruleset={WFD} assistanceOn={LIVE} showAssistance />)
    expect(advisories()).toHaveLength(0)
  })

  it('positive control: a restricted fixture with the same live sources IS visible', () => {
    const restricted: FdRulesetDto = { ...WFD, clusterAllowed: false, spottingAllowed: false }
    render(<FdAdvisories fdActive ruleset={restricted} assistanceOn={LIVE} showAssistance />)
    const cluster = document.querySelector('.fd-advisory.cluster')
    const spotting = document.querySelector('.fd-advisory.spotting')
    expect(cluster, 'cluster restricted + cluster live ⇒ advisory').not.toBeNull()
    expect(cluster!.textContent).toContain('DX cluster / RBN')
    expect(cluster!.textContent).toContain('2026')
    expect(spotting, 'spotting restricted + spot feeds live ⇒ advisory').not.toBeNull()
    expect(spotting!.textContent).toContain('PSK Reporter needs')
  })

  it('restricted but nothing relevant live ⇒ invisible (the advisory cites live sources, never hypothetical ones)', () => {
    const restricted: FdRulesetDto = { ...WFD, clusterAllowed: false, spottingAllowed: false }
    render(<FdAdvisories fdActive ruleset={restricted} assistanceOn={[]} showAssistance />)
    expect(advisories()).toHaveLength(0)
  })

  it('never renders on a surface that did not opt in (the cockpit header hosts only the banned chip)', () => {
    const restricted: FdRulesetDto = { ...WFD, clusterAllowed: false }
    render(<FdAdvisories fdActive ruleset={restricted} assistanceOn={LIVE} />)
    expect(advisories()).toHaveLength(0)
  })
})

describe('the spots-over-RF-only line (Winter Field Day 2027: "You may spot yourself and others only via amateur RF.")', () => {
  const rfOnly: FdRulesetDto = { ...WFD, rulesYear: 2027, spotsRfOnly: true }
  const window = { start: 1_800_720_000, end: 1_800_828_000 }
  const line = () => advisories().filter((a) => a.classList.contains('spotsRfOnly'))

  it('says, while the event runs, that no spot goes out over the internet and receiving carries on', () => {
    render(<FdAdvisories fdActive ruleset={rfOnly} showAssistance eventWindow={window} nowUnix={window.start + 7200} />)
    expect(line().map((a) => a.textContent)).toEqual([
      '⚠ Winter Field Day allows spots only over amateur RF while it runs: Nexus is sending no PSK Reporter reports, DX cluster spots or POTA self-spots until it ends. Receiving spots carries on.',
    ])
  })

  it('is quiet before the event starts and once it has ended', () => {
    for (const now of [window.start - 1, window.end]) {
      render(<FdAdvisories fdActive ruleset={rfOnly} showAssistance eventWindow={window} nowUnix={now} />)
      expect(line(), `at ${now}`).toEqual([])
      cleanup()
    }
  })

  it('CONTROL — a contest without the rule, and the master switch off, say nothing', () => {
    render(<FdAdvisories fdActive ruleset={SFD} showAssistance eventWindow={window} nowUnix={window.start + 7200} />)
    expect(line()).toEqual([])
    cleanup()
    render(<FdAdvisories fdActive={false} ruleset={rfOnly} showAssistance eventWindow={window} nowUnix={window.start + 7200} />)
    expect(line()).toEqual([])
  })
})
