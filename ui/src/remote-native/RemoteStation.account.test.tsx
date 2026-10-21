// @vitest-environment jsdom
// Settings ▸ Remote access, the account's part: signing in at the shack with no code, the notice of a device
// the station granted on its own proof, what the card says of a proof it could not confirm or a removal the
// service has not heard yet, and People.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { AccountStatus, RemoteStationAction, RemoteStationStatus } from './types'
import { EN } from '../i18n/en'

vi.setConfig({ testTimeout: 15_000 })
afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__ })

const noAccount: AccountStatus = { signingIn: false, added: [], unconfirmed: {}, removals: [], people: [] }
const bound = (account: Partial<AccountStatus>, devices: RemoteStationStatus['devices'] = []): RemoteStationStatus => ({
  phase: 'connected', origin: 'https://remote.hamradiotools.io', stationId: crypto.randomUUID(), accountId: crypto.randomUUID(),
  pairingId: null, pairingCode: null, expiresAt: null, devices, error: null, account: { ...noAccount, ...account } })

/** The card over a station whose status `next` answers each action with, recording the actions. */
function shack(first: RemoteStationStatus, next: (action: RemoteStationAction, status: RemoteStationStatus) => RemoteStationStatus = (_, s) => s) {
  let status = first
  const actions: RemoteStationAction[] = []
  window.__TAURI_INTERNALS__ = { invoke: (async (command: string, input: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    actions.push(action)
    status = next(action, status)
    return status
  }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  return actions
}

it('signs in at the shack with the station name, waits for the browser, and can cancel', async () => {
  const unpaired: RemoteStationStatus = { phase: 'unpaired', origin: 'https://remote.hamradiotools.io', stationId: null, accountId: null,
    pairingId: null, pairingCode: null, expiresAt: null, devices: [], error: null, account: noAccount }
  const actions = shack(unpaired, (action, status) => action.type === 'signIn' ? { ...status, account: { ...noAccount, signingIn: true } }
    : action.type === 'cancel' ? { ...status, account: noAccount } : status)
  const signIn = await screen.findByRole('button', { name: EN['remote.account.signIn'] }) as HTMLButtonElement
  expect(signIn.disabled).toBe(true)
  // Pairing with a code stays beside it.
  expect(screen.getByRole('button', { name: EN['remote.pairStation'] })).toBeTruthy()
  fireEvent.change(screen.getByLabelText(EN['remote.stationName']), { target: { value: 'Shack' } })
  fireEvent.click(signIn)
  await screen.findByText(EN['remote.account.signingIn'])
  expect(screen.queryByRole('button', { name: EN['remote.pairStation'] })).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.cancelSignIn'] }))
  await screen.findByRole('button', { name: EN['remote.account.signIn'] })
  expect(actions).toEqual([{ type: 'signIn', name: 'Shack' }, { type: 'cancel' }])
})

it('says why signing in is not offered by this build', async () => {
  shack({ ...bound({}), phase: 'unpaired', stationId: null, error: 'accountNotReady' })
  await screen.findByText(EN['remote.account.notReady'])
})

it('shows a device the station granted on its own proof until the operator has seen it, with Remove beside it', async () => {
  const deviceId = crypto.randomUUID(), key = 'a1b2c3d4'.repeat(8)
  const added = { deviceId, name: 'DESKTOP-7Q', kind: 'computer' as const, platform: 'windows' as const, key, role: 'operate' as const, guest: false }
  const actions = shack(bound({ added: [added] }), (action, status) => action.type === 'dismissAdded' ? { ...status, account: noAccount } : status)
  const notice = await screen.findByText(/DESKTOP-7Q \(Nexus on Windows\), key A1B2/)
  expect(notice.textContent).toMatch(/^Added by signing in as you/)
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.gotIt'] }))
  await waitFor(() => expect(screen.queryByText(/DESKTOP-7Q/)).toBeNull())
  expect(actions).toEqual([{ type: 'dismissAdded', deviceId }])
})

it('removes a granted device straight from its notice', async () => {
  const deviceId = crypto.randomUUID()
  const added = { deviceId, name: 'K9 laptop', kind: 'browser' as const, key: null, role: 'listen' as const, guest: true }
  const actions = shack(bound({ added: [added] }))
  await screen.findByText(/Added for someone you invited \(Listen\): K9 laptop \(a browser\)/)
  fireEvent.click(screen.getByRole('button', { name: EN['remote.native.remove'] }))
  await waitFor(() => expect(actions).toEqual([{ type: 'device', deviceId, approve: false }]))
})

it('says when the station could not confirm a device the service approved, and when a removal waits', async () => {
  const unconfirmed = crypto.randomUUID(), removed = crypto.randomUUID(), key = 'ab'.repeat(32)
  const devices = [
    { id: unconfirmed, name: 'Unknown browser', approved: 1, expiresAt: Date.now() + 600000, key, addedBy: 'account' as const },
    { id: removed, name: 'Old browser', approved: 1, expiresAt: Date.now() + 600000, key },
  ]
  shack({ ...bound({ unconfirmed: { [unconfirmed]: 'proofNotYours' }, removals: [removed] }, devices), pinnedDevices: [] })
  await screen.findByText(EN['remote.account.unconfirmed'])
  expect(screen.getByText(EN['remote.account.removalWaiting'])).toBeTruthy()
  expect(screen.getByText(EN['remote.account.addedByAccount'])).toBeTruthy()
  // The prompt a device added at the station gets is still there: approving it is the operator's call.
  expect(screen.getAllByRole('button', { name: EN['remote.approve'] }).length).toBe(2)
})

// A device added by signing in is listed approved by the service, and the station grants it nothing until it is approved
// here (`remote_service::account::GRANTS_ON_SIGN_IN`): its row asks, as a device added at the station asks, with Approve, and
// never says the station could not confirm it (it could).
it('shows a device added by signing in as asking, with Approve, until it is approved here', async () => {
  const computer = crypto.randomUUID(), browser = crypto.randomUUID(), key = 'ab'.repeat(32)
  const devices = [
    { id: computer, name: 'DESKTOP-7Q', approved: 1, expiresAt: Date.now() + 600000, key, kind: 'computer' as const,
      platform: 'windows' as const, addedBy: 'account' as const },
    { id: browser, name: 'Chrome on Windows', approved: 1, expiresAt: Date.now() + 600000, key, addedBy: 'account' as const },
  ]
  const actions = shack({ ...bound({ unconfirmed: { [computer]: 'approveAtStation', [browser]: 'approveAtStation' } }, devices),
    pinnedDevices: [] })
  await screen.findByText(EN['remote.native.computerAsks'])
  expect(screen.getByText(EN['remote.native.asks'])).toBeTruthy()
  expect(screen.queryByText(EN['remote.account.unconfirmed']), 'it was confirmed').toBeNull()
  expect(screen.queryByText(EN['remote.native.computerKeyNotPinned']), 'never approved here').toBeNull()
  expect(screen.getAllByText(EN['remote.account.addedByAccount']).length).toBe(2)
  fireEvent.click(screen.getAllByRole('button', { name: EN['remote.approve'] })[1])
  await waitFor(() => expect(actions).toEqual([{ type: 'device', deviceId: computer, approve: true, transmit: false, key, kind: 'computer' }]))
})

it('invites someone with a role, lists the people invited, and takes an invitation back', async () => {
  const actions = shack(bound({ people: [{ mailbox: 'k9.guest@example.test', role: 'listen' }] }))
  const person = await screen.findByText('k9.guest@example.test')
  expect(person.parentElement!.textContent).toContain(EN['remote.account.role.listen'])
  const invite = screen.getByRole('button', { name: EN['remote.account.invite'] }) as HTMLButtonElement
  expect(invite.disabled).toBe(true)
  fireEvent.change(screen.getByLabelText(EN['remote.account.email']), { target: { value: ' W1AW@example.test ' } })
  fireEvent.change(screen.getByLabelText(EN['remote.account.role']), { target: { value: 'listen' } })
  fireEvent.click(invite)
  await waitFor(() => expect(actions).toEqual([{ type: 'invite', email: 'W1AW@example.test', role: 'listen' }]))
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.uninvite'] }))
  await waitFor(() => expect(actions[actions.length - 1]).toEqual({ type: 'uninvite', email: 'k9.guest@example.test' }))
})

it('asks a bound station to sign in again once its proof key is retired, and waits for the browser', async () => {
  const actions = shack(bound({ reproof: true }), (action, status) => action.type === 'signInAgain'
    ? { ...status, account: { ...status.account!, signingIn: true } }
    : action.type === 'cancelSignInAgain' ? { ...status, account: { ...status.account!, signingIn: false } } : status)
  await screen.findByText(EN['remote.account.reproof'])
  fireEvent.click(await screen.findByRole('button', { name: EN['remote.account.signInAgain'] }))
  await screen.findByText(EN['remote.account.signingIn'])
  expect(actions[actions.length - 1]).toEqual({ type: 'signInAgain' })
  expect(screen.queryByRole('button', { name: EN['remote.account.signInAgain'] })).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.cancelSignIn'] }))
  await screen.findByRole('button', { name: EN['remote.account.signInAgain'] })
  expect(actions[actions.length - 1]).toEqual({ type: 'cancelSignInAgain' })
})

it('offers no second sign-in while the proof still holds, and names a sign-in by someone else', async () => {
  shack({ ...bound({}), error: 'proofNotYours' })
  await screen.findByText(EN['remote.account.notOwner'])
  expect(screen.queryByRole('button', { name: EN['remote.account.signInAgain'] })).toBeNull()
})
