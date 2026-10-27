// The radio switch notice: one line when Nexus switches the active radio BY ITSELF ("Switched to
// IC-9700 for 2m · Weak-signal digital"), with Undo and Why?.
//
// What raises it is the engine's own record of the switch (`AppSnapshot.autoRadioSwitch`, written
// only in the routing branch of a tune), never a guess from the active id changing: the operator's
// own radio pick, a Remote selection and a radio disabled in Settings change the active id too,
// and none of those is Nexus acting by itself.
//
// Shown only when the station's policy is `tell` (a new install's default); an existing settings
// file keeps the silent switch it always had until the operator changes it in Settings ▸ Radio ▸
// Radios. The switch itself is the same either way: this only says it happened.
import { useEffect, useRef } from 'react'
import { t } from './i18n'
import { dismissToast, pushToast } from './toast'
import { ROUTE_MODE_LABEL } from './settings/describeRouting'
import type { AppSnapshot, RadioSwitchPolicy } from './types'

/** How long the notice stays, ms. Long enough to read and reach Undo from the rig. */
export const RADIO_SWITCH_NOTICE_MS = 12_000

export interface RadioSwitchNoticeDeps {
  snap: Pick<AppSnapshot, 'autoRadioSwitch' | 'radios'> | null | undefined
  /** The station's policy; absent (settings not read yet) shows nothing. */
  policy: RadioSwitchPolicy | undefined
  /** Select a radio again through the operator's own radio pick (`set_active_radio`). A rejection
   *  keeps the notice up with the reason, so Undo can be pressed again. */
  selectRadio: (id: number) => Promise<unknown>
  /** Open Settings at a section id (App's `openSettingsAt`). */
  openSettingsAt: (where: string) => void
}

export function useRadioSwitchNotice({ snap, policy, selectRadio, openSettingsAt }: RadioSwitchNoticeDeps): void {
  // The last switch already accounted for. `undefined` until the first snapshot: a switch made
  // before this window opened (a reload, a pop-out) is history, not news.
  const seen = useRef<number | null | undefined>(undefined)
  const shown = useRef<number | null>(null)
  const latest = useRef({ selectRadio, openSettingsAt })
  latest.current = { selectRadio, openSettingsAt }
  const sw = snap?.autoRadioSwitch
  const seq = snap ? (sw?.seq ?? null) : undefined

  useEffect(() => {
    if (seq === undefined) return
    if (seen.current === undefined) {
      seen.current = seq
      return
    }
    if (seq === null || seq === seen.current || !sw) return
    seen.current = seq
    if (policy !== 'tell') return
    const name = (id: number) => snap?.radios?.find((r) => r.id === id)?.name ?? t('radioSwitch.radio', { id })
    const mode = ROUTE_MODE_LABEL[sw.mode] ?? sw.mode
    const message = sw.band
      ? t('radioSwitch.notice', { radio: name(sw.toId), band: sw.band, mode })
      : t('radioSwitch.notice.noBand', { radio: name(sw.toId), mode })
    // One notice at a time: a newer switch replaces the one still up.
    if (shown.current != null) dismissToast(shown.current)
    const from = sw.fromId
    shown.current = pushToast(message, 'info', RADIO_SWITCH_NOTICE_MS, {
      action: () => latest.current.selectRadio(from),
      actionLabel: t('radioSwitch.undo'),
      secondaryAction: () => latest.current.openSettingsAt('radios'),
      secondaryLabel: t('radioSwitch.why'),
    })
    // `snap` and `sw` are read for the switch `seq` names; a new switch is a new seq.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seq, policy])
}
