/**
 * Settings ▸ Digital ▸ JS8 ▸ JS8Call API: JS8Call's API for other programs on THIS computer, two
 * switches and their ports, with JS8Call's own defaults (both off; TCP 2442, UDP 2242).
 *
 * What the words below promise, and the code that keeps the promise: a program written for
 * JS8Call can READ what the station hears, its inbox and its station details, and nothing else —
 * every request that would transmit, tune or change a setting is dropped unanswered
 * (`tempo_app::engine::js8_api`) — and only a program on this computer can reach it: both sides
 * are bound to 127.0.0.1 whatever the port (`src-tauri/src/js8_api.rs`). Neither setting reaches a
 * Remote browser, so SettingsPanel renders this at the station only.
 */
import { Fragment } from 'react'
import { t } from '../i18n'
import type { Settings } from '../types'

type Switch = 'js8ApiTcp' | 'js8ApiUdp'
type Port = 'js8ApiTcpPort' | 'js8ApiUdpPort'

export interface Js8ApiSettingsProps {
  form: Settings
  onSwitch: (key: Switch, on: boolean) => void
  onPort: (key: Port, port: number) => void
}

/** A typed port, 1–65535; anything else is no port and leaves the field as it was. */
export function parsePort(raw: string): number | null {
  const n = Number(raw)
  return raw.trim() !== '' && Number.isInteger(n) && n >= 1 && n <= 65535 ? n : null
}

export function Js8ApiSettings({ form, onSwitch, onPort }: Js8ApiSettingsProps) {
  const sides: {
    on: Switch
    port: Port
    fallback: number
    label: string
    hint: string
    portLabel: string
    portHint: string
  }[] = [
    {
      on: 'js8ApiTcp',
      port: 'js8ApiTcpPort',
      fallback: 2442,
      label: t('settings.js8.api.tcp.label'),
      hint: t('settings.js8.api.tcp.hint'),
      portLabel: t('settings.js8.api.tcpPort.label'),
      portHint: t('settings.js8.api.tcpPort.hint'),
    },
    {
      on: 'js8ApiUdp',
      port: 'js8ApiUdpPort',
      fallback: 2242,
      label: t('settings.js8.api.udp.label'),
      hint: t('settings.js8.api.udp.hint'),
      portLabel: t('settings.js8.api.udpPort.label'),
      portHint: t('settings.js8.api.udpPort.hint'),
    },
  ]
  return (
    <div className="settings-featgroup">
      <span className="settings-featgroup-title">{t('settings.js8.api.title')}</span>
      <span className="settings-hint">{t('settings.js8.api.hint')}</span>
      {sides.map((side) => {
        const on = form[side.on] === true
        return (
          <Fragment key={side.on}>
            <div className="settings-field">
              <label className="settings-toggle">
                <span className="settings-label">{side.label}</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={on}
                  aria-label={side.label}
                  className={`toggle${on ? ' on' : ''}`}
                  onClick={() => onSwitch(side.on, !on)}
                >
                  <span className="toggle-knob" />
                </button>
              </label>
              <span className="settings-hint">{side.hint}</span>
            </div>
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
          </Fragment>
        )
      })}
    </div>
  )
}
