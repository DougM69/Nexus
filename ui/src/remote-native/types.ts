export type RemoteStationStatus = {
  phase: 'unpaired' | 'pairing' | 'approval' | 'disabled' | 'connecting' | 'connected' | 'reconnecting'
  origin: string; stationId: string | null; accountId: string | null
  pairingId: string | null; pairingCode: string | null; expiresAt: number | null
  /** `renewsUntil` is the end that use cannot move; null (or absent) for an approval that never renews.
   *  `key` is the SHA-256 of the browser's listed device key (A5), lowercase hex, when it has one.
   *  `kind`: a browser, or Nexus itself on another computer or a phone (the native client), whose
   *  approval pins its key and grants nothing on the relay's lanes; absent from an older station, where
   *  every device is a browser. `platform`: a native device's, none for a browser. `untilRemoved`: 1 for an
   *  approval that lasts until it is removed here (every approval since 2026-10-10); absent from an older
   *  station or service. */
  devices: { id: string; name: string; approved: number; expiresAt: number; generation?: number | null; renewsUntil?: number | null
    key?: string | null; kind?: DeviceKind; platform?: DevicePlatform | null; untilRemoved?: number }[]
  loggingPermissions?: string[]
  stationPermissions?: string[]
  transmitPermissions?: string[]
  loggingController?: string | null
  /** A5: the browsers whose listed key is the one pinned here, the ones that can stream. */
  pinnedDevices?: string[]
  /** S3-M1: the service holds another signing key for this station, so browsers refuse its stream
   *  until it is paired again. */
  keyRefused?: boolean
  /** S3-L1: SHA-256 of this station's own key, lowercase hex, while it has one: shown as "This
   *  station's key", the way the Remote page shows the key it kept for the station. */
  stationKey?: string | null
  observationGeneration?: string | null
  error: string | null
  /** Remote over this network: absent only from a build without it. */
  lan?: LanStatus
}
/** Remote over this network (`Status.lan`): the shack's own listener, its paired computers and its
 *  pairing window. `reason`: why it went off by itself, or why it is on and not listening
 *  (`addressGone`: the picked address is not this computer's right now, and is waited for). */
export type LanStatus = {
  on: boolean
  /** Where it listens, `address:port`. */
  listening?: string
  reason?: 'noKey' | 'endedAtShack' | 'addressGone' | 'chooseAddress' | 'portInUse' | 'noNetwork' | 'unavailable'
  /** While on, the networks to pick from: this computer's private addresses on adapters that are
   *  not tunnels or virtual ones, each with its adapter's name. */
  networks?: { address: string; name: string }[]
  /** The address the operator picked, if any. */
  picked?: string
  /** While listening: Windows advertises the station by name (`true`) or will not (`false`). */
  named?: boolean
  /** While listening, what stands in the way in Windows' firewall on that network. */
  firewall?: 'blocksAll' | 'blocked' | 'managed' | 'public' | 'silent' | 'ask'
  /** The LAN key's fingerprint: SHA-256 of its SPKI, lowercase hex. */
  key?: string
  /** The pairing window while it is open: its code (sixteen lowercase hex characters) and when it
   *  closes, in milliseconds since 1970. */
  pairing?: { code: string; closesAt: number }
  /** Each paired computer: its device id, the name it gave, and its key's fingerprint. */
  devices: { id: string; name: string; key: string }[]
}
/** A device's kind and a native device's platform, as the station lists them (the service's own values). */
export type DeviceKind = 'browser' | 'computer' | 'phone'
export type DevicePlatform = 'windows' | 'macos' | 'linux' | 'ios' | 'android'
/** The platforms by name: proper nouns, the same in every language. */
export const PLATFORM_NAMES: Record<DevicePlatform, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux', ios: 'iOS', android: 'Android' }
/** Nexus on another computer or a phone (the native client), not a browser. */
export const isNative = (device: { kind?: DeviceKind }) => device.kind === 'computer' || device.kind === 'phone'
export type RemoteStationAction =
  | { type: 'begin'; name: string }
  | { type: 'refresh' | 'cancel' | 'enable' | 'disable' | 'forget' }
  | { type: 'approve'; enrollmentId: string; accountId: string; transmit?: boolean }
  | { type: 'loggingPermission'; deviceId: string; allow: boolean }
  | { type: 'stationPermission'; deviceId: string; allow: boolean }
  | { type: 'transmitPermission'; deviceId: string; allow: boolean }
  | { type: 'takeOverLogging' }
  /** `key`: the device key shown beside the browser, pinned if the service still lists it (A5). `kind`: the
   *  kind it was shown as; the station grants and pins nothing for a device the service now names another. */
  | { type: 'device'; deviceId: string; approve: boolean; transmit?: boolean; key?: string; kind?: DeviceKind }
  /** Remote over this network: only ever pressed at the shack (`LanStation`). */
  | { type: 'lanOn'; address?: string; port?: number }
  /** Where it listens: one of `networks`' addresses, or none to let the station choose. */
  | { type: 'lanAddress'; address?: string }
  | { type: 'lanOff' | 'lanPair' | 'lanCancel' | 'lanReset' }
  | { type: 'lanRevoke'; deviceId: string }
