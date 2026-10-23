// @vitest-environment jsdom
//
// A LOGBOOK GLOBE THAT FAILS ON SCREEN COSTS THE BAND, NEVER THE LOGBOOK.
//
// The probe can pass and three.js still fail to make its context: the browser's cap on live contexts, a GPU
// process that died, a driver that refuses. A lost context that never came back paused the globe with a Reload
// (globeWebgl.tsx), and a Reload that could not make a context threw out of the band into App's section
// boundary: the whole Logbook became the error page. The globe is in a boundary of its own now; the band says
// the globe stopped, and the table, its search and its forms carry on.
//
// jsdom has no WebGL: `react-globe.gl` is a stub that mounts in a layout effect as react-kapsule does, and
// throws there as three.js does when it cannot make its context. The Logbook, QsoGlobe and the GPU probes
// are the real ones; the canvas contexts the probes see are fakes of a capable GPU.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef } from 'react'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import * as THREE from 'three'
import type { LogQuestion } from '../features/logAnswers'

const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((fn: () => unknown) => fn()) }))
// jsdom loads no images: the day picture is not what is under test.
vi.mock('../features/globeBasemap', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/globeBasemap')>()),
  loadDayImage: () => Promise.resolve(null),
}))

/** The machine: whether three.js can make a context right now, and each globe mounted, oldest first. */
const machine = vi.hoisted(() => ({ noContext: false, globes: [] as Array<{ lose(): void }> }))
vi.mock('react-globe.gl', () => ({
  default: forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    const host = useRef<HTMLDivElement>(null)
    const inst = useMemo(() => {
      const canvas = document.createElement('canvas')
      let lost = false
      const scene = new THREE.Scene()
      const api = {
        scene: () => scene,
        lights: () => [],
        renderer: () => ({
          domElement: canvas,
          capabilities: { maxTextureSize: 4096, getMaxAnisotropy: () => 1 },
          getContext: () => ({ isContextLost: () => lost }),
          dispose() {},
          forceContextLoss() {
            lost = true
          },
        }),
        controls: () => ({ autoRotate: false, autoRotateSpeed: 0, connect() {}, disconnect() {} }),
        camera: () => new THREE.PerspectiveCamera(),
        getCoords: () => ({ x: 0, y: 0, z: 0 }),
        pauseAnimation() {},
        resumeAnimation() {},
      }
      const lose = () => {
        lost = true
        canvas.dispatchEvent(new Event('webglcontextlost', { cancelable: true }))
      }
      return { canvas, api, lose }
    }, [])
    useImperativeHandle(ref, () => inst.api, [inst])
    // react-kapsule mounts globe.gl in a layout effect, and that is where three.js makes its context.
    useLayoutEffect(() => {
      if (machine.noContext) throw new Error('THREE.WebGLRenderer: Error creating WebGL context.')
      host.current!.appendChild(inst.canvas)
      machine.globes.push(inst)
    }, [inst])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" ref={host} />
  }),
}))

import { Logbook } from './Logbook'
import { ErrorBoundary } from './ErrorBoundary'
import { RESTORE_WAIT_MS } from './globeWebgl'
import { t } from '../i18n'

const UNMASKED_RENDERER_WEBGL = 0x9246
const MAX_TEXTURE_SIZE = 0x0d33
/** A WebGL context on a capable GPU, for the probes (WebGL 2 and WebGL 1 alike). */
const capable = () => ({
  MAX_TEXTURE_SIZE,
  getExtension: (name: string) =>
    name === 'WEBGL_debug_renderer_info'
      ? { UNMASKED_RENDERER_WEBGL }
      : name === 'WEBGL_lose_context'
        ? { loseContext() {}, restoreContext() {} }
        : null,
  getParameter: (p: number) =>
    p === UNMASKED_RENDERER_WEBGL ? 'ANGLE (Intel, Mesa Intel(R) UHD Graphics 620 (KBL GT2), OpenGL 4.6)' : p === MAX_TEXTURE_SIZE ? 16384 : null,
  isContextLost: () => false,
})

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  globalThis.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof IntersectionObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
beforeEach(() => {
  localStorage.clear()
  machine.noContext = false
  machine.globes.length = 0
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    /webgl/.test(kind) ? capable() : null) as never)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(900)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(320)
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  vi.useRealTimers()
  cleanup()
  vi.restoreAllMocks()
})

const qso = (call: string, i: number) => ({
  call, grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', name: null,
  qth: null, comment: null, notes: null, country: 'United States', whenUnix: 1_700_000_000 + i, confirmed: false,
  awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null, upload: undefined,
})
const LOG = [qso('K1ABC', 0), qso('K2XYZ', 1), qso('W3DEF', 2)]

const calls = (c: HTMLElement) =>
  [...c.querySelectorAll('button[aria-label^="Edit "]')].map((b) => b.getAttribute('aria-label')!.slice(5)).sort()
const crash = (c: HTMLElement) => c.querySelector('.view-crash')?.textContent ?? null
const band = (c: HTMLElement) => c.querySelector('.log-globe-band')

/** The Logbook as App mounts it, inside App's section boundary, with its rows and its log size in. */
async function mount() {
  engineLog.mockResolvedValue(LOG)
  const { container } = render(
    <ErrorBoundary label={t('logbook.title')}>
      <Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />
    </ErrorBoundary>,
  )
  await waitFor(() => expect(calls(container)).toEqual(['K1ABC', 'K2XYZ', 'W3DEF']))
  await waitFor(() => expect(container.querySelector('.count-badge')?.textContent).toBe('3'))
  return container
}

/** Until the band holds a globe, a note, or the section has become its error page. */
async function settled(c: HTMLElement) {
  await waitFor(() => expect(c.querySelector('.view-crash') ?? band(c)?.querySelector('.globe-note, canvas')).toBeTruthy(), {
    timeout: 10_000,
  })
}

/** The table still answers: the search narrows it and clearing it brings every row back. */
async function tableWorks(c: HTMLElement) {
  fireEvent.change(c.querySelector('.log-search') as HTMLInputElement, { target: { value: 'W3' } })
  await waitFor(() => expect(calls(c)).toEqual(['W3DEF']))
  fireEvent.change(c.querySelector('.log-search') as HTMLInputElement, { target: { value: '' } })
  await waitFor(() => expect(calls(c)).toEqual(['K1ABC', 'K2XYZ', 'W3DEF']))
}

describe('a Logbook globe that fails on screen', () => {
  it('POSITIVE CONTROL: on a capable GPU the band draws the globe', async () => {
    const c = await mount()
    await settled(c)
    expect(crash(c)).toBeNull()
    expect(band(c)?.querySelector('canvas')).not.toBeNull()
    expect(machine.globes).toHaveLength(1)
  })

  it('three.js unable to make its context as the globe mounts: the band says the globe stopped, the table works', async () => {
    machine.noContext = true
    const c = await mount()
    await settled(c)
    expect(crash(c), 'the Logbook is the error page').toBeNull()
    expect(band(c)?.textContent).toBe(t('logbook.globe.failed'))
    await tableWorks(c)
  })

  it('a context lost for good, and a Reload that cannot make one: the band says so, the table works', async () => {
    const c = await mount()
    await settled(c)
    expect(machine.globes).toHaveLength(1)
    vi.useFakeTimers()
    await act(async () => {
      machine.globes[0].lose()
      vi.advanceTimersByTime(RESTORE_WAIT_MS)
    })
    vi.useRealTimers()
    const reload = screen.getByRole('button', { name: t('globe.paused.reload') })
    machine.noContext = true
    await act(async () => reload.click())
    await settled(c)
    expect(crash(c), 'the Logbook is the error page').toBeNull()
    expect(band(c)?.textContent).toBe(t('logbook.globe.failed'))
    await tableWorks(c)
  })
})
