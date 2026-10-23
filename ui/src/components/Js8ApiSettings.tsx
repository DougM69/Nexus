/**
 * Settings ▸ Digital ▸ JS8 ▸ JS8Call API: JS8Call's API for other programs on THIS computer, two
 * switches and their ports, and each side's "Accept requests", with JS8Call's own defaults (all
 * off; TCP 2442, UDP 2242).
 *
 * What the words below promise, and the code that keeps the promise: a program written for
 * JS8Call can READ what the station hears, its inbox and its station details, and nothing else —
 * every request that would transmit, tune or change a setting is dropped unanswered
 * (`tempo_app::engine::js8_api`), and with a side's "Accept requests" off (JS8Call's default) none
 * of its requests is answered at all — and only a program on this computer can reach it: both
 * sides are bound to 127.0.0.1 whatever the port (`src-tauri/src/js8_api.rs`). None of these
 * settings reaches a Remote browser, so SettingsPanel renders this at the station only.
 */
import { Fragment, type ReactNode } from 'react'
import { t } from '../i18n'
import type { Settings } from '../types'

type Switch = 'js8ApiTcp' | 'js8ApiUdp' | 'js8ApiTcpAccept' | 'js8ApiUdpAccept'
type Port = 'js8ApiTcpPort' | 'js8ApiUdpPort'

export interface Js8ApiSettingsProps {
  form: Settings
  onSwitch: (key: Switch, on: boolean) => void
  onPort: (key: Port, port: number) => void
  /** From a computer driving the station: a switch the save from there keeps as the station has
   *  it shows read-only, and `setAtStation` says so. Absent at the station. */
  locked?: (key: Switch) => boolean
  setAtStation?: (key: Switch) => ReactNode
}

/** A typed port, 1–65535; anything else is no port and leaves the field as it was. */
export function parsePort(raw: string): number | null {
  const n = Number(raw)
  return raw.trim() !== '' && Number.isInteger(n) && n >= 1 && n <= 65535 ? n : null
}

export function Js8ApiSettings({ form, onSwitch, onPort, locked, setAtStation }: Js8ApiSettingsProps) {
  const sides: {
    on: Switch
    port: Port
    accept: Switch
    fallback: number
    label: string
    hint: string
    portLabel: string
    portHint: string
    acceptLabel: string
  }[] = [
    {
      on: 'js8ApiTcp',
      port: 'js8ApiTcpPort',
      accept: 'js8ApiTcpAccept',
      fallback: 2442,
      label: t('settings.js8.api.tcp.label'),
      hint: t('settings.js8.api.tcp.hint'),
      portLabel: t('settings.js8.api.tcpPort.label'),
      portHint: t('settings.js8.api.tcpPort.hint'),
      acceptLabel: t('settings.js8.api.tcpAccept.label'),
    },
    {
      on: 'js8ApiUdp',
      port: 'js8ApiUdpPort',
      accept: 'js8ApiUdpAccept',
      fallback: 2242,
      label: t('settings.js8.api.udp.label'),
      hint: t('settings.js8.api.udp.hint'),
      portLabel: t('settings.js8.api.udpPort.label'),
      portHint: t('settings.js8.api.udpPort.hint'),
      acceptLabel: t('settings.js8.api.udpAccept.label'),
    },
  ]
  const toggle = (key: Switch, label: string, hint: string) => {
    const on = form[key] === true
    return (
      <div className="settings-field">
        <label className="settings-toggle">
          <span className="settings-label">{label}</span>
          <button disabled={locked?.(key)} data-setting={key}
            type="button"
            role="switch"
            aria-checked={on}
            aria-label={label}
            className={`toggle${on ? ' on' : ''}`}
            onClick={() => onSwitch(key, !on)}
          >
            <span className="toggle-knob" />
          </button>
        </label>
        {setAtStation?.(key)}
        <span className="settings-hint">{hint}</span>
      </div>
    )
  }
  return (
    <div className="settings-featgroup">
      <span className="settings-featgroup-title">{t('settings.js8.api.title')}</span>
      <span className="settings-hint">{t('settings.js8.api.hint')}</span>
      {sides.map((side) => {
        return (
          <Fragment key={side.on}>
            {toggle(side.on, side.label, side.hint)}
            <label className="settings-field">
              <span className="settings-label">{side.portLabel}</span>
              <input
                className="settings-input"
                type="number"
                inputMode="numeric"
                min={1}
                max={65535}
                value={String(form[side.port] ?? side.fallback)}
                placeholder={String(side.fallback)}
                onChange={(e) => {
                  const port = parsePort(e.target.value)
                  if (port !== null) onPort(side.port, port)
                }}
                autoComplete="off"
              />
              <span className="settings-hint">{side.portHint}</span>
            </label>
            {toggle(side.accept, side.acceptLabel, t('settings.js8.api.accept.hint'))}
          </Fragment>
        )
      })}
    </div>
  )
}
