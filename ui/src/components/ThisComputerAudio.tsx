// This computer ▸ Audio: the headset a computer driving a station (client mode) hears the station's
// audio on and talks into, and how loud the station is heard here (the hosted page's volume, 0 to
// 42 dB over the station's own level). The choice belongs to this computer and is kept here, in a
// file of its own, never sent to the station (`set_client_audio`). A meter shows the microphone's
// level while the section is open, and a test plays a steady note in the output chosen.
//
// The microphone is opened only while something needs it (this meter, or a press), so a computer
// that is only listening never holds it. A device that will not open says why, in Nexus's words
// for the system's own reason; it is never swapped for the system's default behind the operator's
// back.

import { useEffect, useState } from 'react'
import {
  clientAudio,
  clientAudioLevel,
  clientAudioTest,
  clientStatus,
  isTauri,
  setClientAudio,
  type ClientAudio,
  type ClientAudioDevice,
} from '../api'
import { t } from '../i18n'
import { AUDIO_VOLUME_DB } from '../remote-web/audio-worklet'

/** How often the meter asks after the microphone's level while it shows. */
export const METER_EVERY_MS = 100

/** Whether this computer drives a station (client mode), asked once: the section shows only then. */
export function useDriving(): boolean {
  const [driving, setDriving] = useState(false)
  useEffect(() => {
    if (!isTauri()) return
    let alive = true
    void clientStatus()
      .then((status) => {
        if (alive) setDriving(status !== null)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])
  return driving
}

function Choice({
  label,
  hint,
  value,
  devices,
  onPick,
}: {
  label: string
  hint: string
  value: string
  devices: ClientAudioDevice[]
  onPick: (name: string) => void
}) {
  // A device chosen earlier that is not plugged in now stays shown, so the choice is never
  // silently replaced by another.
  const listed = value === '' || devices.some((d) => d.name === value)
  return (
    <div className="settings-field">
      <span className="settings-label">{label}</span>
      <select
        className="settings-input"
        value={value}
        onChange={(e) => onPick(e.target.value)}
        aria-label={label}
      >
        <option value="">{t('settings.thisComputer.audio.default')}</option>
        {!listed && <option value={value}>{t('settings.thisComputer.audio.missing', { name: value })}</option>}
        {devices.map((d) => (
          <option key={d.name} value={d.name}>
            {d.label}
          </option>
        ))}
      </select>
      <span className="settings-hint">{hint}</span>
    </div>
  )
}

/** The volume as gain over the station's own level, built invariantly: dB is not translated. */
function volumeReading(db: number): string {
  return `+${db} dB`
}

export function ThisComputerAudio() {
  const [audio, setAudio] = useState<ClientAudio | null>(null)
  const [notKept, setNotKept] = useState(false)
  const [level, setLevel] = useState(0)
  const [micTrouble, setMicTrouble] = useState<string | null>(null)
  const [test, setTest] = useState<'idle' | 'playing' | 'played' | 'failed'>('idle')
  const [testTrouble, setTestTrouble] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    void clientAudio()
      .then((now) => {
        if (alive) setAudio(now)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  const available = audio?.available ?? false
  useEffect(() => {
    if (!available) return
    let alive = true
    const timer = setInterval(() => {
      void clientAudioLevel()
        .then((now) => {
          if (!alive) return
          setLevel(Math.max(0, Math.min(1, now.level)))
          setMicTrouble(now.trouble)
        })
        .catch(() => {})
    }, METER_EVERY_MS)
    return () => {
      alive = false
      clearInterval(timer)
    }
  }, [available])

  if (!audio) return null
  if (!audio.available) {
    return <p className="settings-note" role="status">{t('settings.thisComputer.audio.unavailable')}</p>
  }
  const pick = (output: string, input: string, volumeDb = audio.volumeDb) => {
    void setClientAudio(output, input, volumeDb)
      .then((now) => {
        setAudio(now)
        setNotKept(false)
      })
      .catch(() => setNotKept(true))
  }
  const play = () => {
    setTest('playing')
    void clientAudioTest()
      .then((now) => {
        setTest(now.played ? 'played' : 'failed')
        setTestTrouble(now.trouble ?? null)
      })
      .catch(() => {
        setTest('failed')
        setTestTrouble(null)
      })
  }
  const outTrouble = audio.trouble.output ?? (test === 'failed' ? testTrouble : null)
  return (
    <>
      <p className="settings-note">{t('settings.thisComputer.audio.note')}</p>
      <Choice
        label={t('settings.thisComputer.audio.output.label')}
        hint={t('settings.thisComputer.audio.output.hint')}
        value={audio.output}
        devices={audio.outputs}
        onPick={(name) => pick(name, audio.input)}
      />
      <div className="settings-field">
        <span className="settings-label">{t('settings.thisComputer.audio.volume.label')}</span>
        <input
          type="range"
          className="settings-input"
          min={AUDIO_VOLUME_DB.min}
          max={AUDIO_VOLUME_DB.max}
          step={1}
          value={audio.volumeDb}
          aria-label={t('settings.thisComputer.audio.volume.label')}
          aria-valuetext={volumeReading(audio.volumeDb)}
          onChange={(e) => pick(audio.output, audio.input, Number(e.currentTarget.value))}
        />
        <span className="settings-hint">{t('settings.thisComputer.audio.volume.hint')}</span>
      </div>
      <div className="settings-field">
        <span className="settings-label">{t('settings.thisComputer.audio.test.label')}</span>
        <button type="button" className="settings-refresh" onClick={play} disabled={test === 'playing'}>
          {t('settings.thisComputer.audio.test')}
        </button>
        {test === 'failed' && (
          <span className="settings-hint" role="alert">
            {outTrouble
              ? t('settings.thisComputer.audio.output.trouble', { why: outTrouble })
              : t('settings.thisComputer.audio.test.failed')}
          </span>
        )}
      </div>
      {test !== 'failed' && audio.trouble.output && (
        <p className="settings-note" role="alert">
          {t('settings.thisComputer.audio.output.trouble', { why: audio.trouble.output })}
        </p>
      )}
      <Choice
        label={t('settings.thisComputer.audio.input.label')}
        hint={t('settings.thisComputer.audio.input.hint')}
        value={audio.input}
        devices={audio.inputs}
        onPick={(name) => pick(audio.output, name)}
      />
      <div className="settings-field">
        <span className="settings-label">{t('settings.thisComputer.audio.meter.label')}</span>
        <meter
          className="settings-input"
          min={0}
          max={1}
          low={0.05}
          high={0.9}
          optimum={0.3}
          value={level}
          aria-label={t('settings.thisComputer.audio.meter.label')}
        />
        <span className="settings-hint">{t('settings.thisComputer.audio.meter.hint')}</span>
      </div>
      {micTrouble && (
        <p className="settings-note" role="alert">
          {t('settings.thisComputer.audio.input.trouble', { why: micTrouble })}
        </p>
      )}
      {notKept && (
        <p className="settings-note" role="alert">
          {t('settings.thisComputer.audio.notKept')}
        </p>
      )}
    </>
  )
}
