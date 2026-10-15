// The compose box's settings ride a config profile like any other: saved with it, loaded back by
// value, and a profile from before them leaves the current ones as they are (profiles.ts's merge).
import { describe, it, expect, beforeEach } from 'vitest'

class MemoryStorage {
  private m = new Map<string, string>()
  get length() {
    return this.m.size
  }
  clear() {
    this.m.clear()
  }
  getItem(k: string) {
    return this.m.has(k) ? (this.m.get(k) as string) : null
  }
  key(i: number) {
    return [...this.m.keys()][i] ?? null
  }
  removeItem(k: string) {
    this.m.delete(k)
  }
  setItem(k: string, v: string) {
    this.m.set(k, String(v))
  }
}
const memStore = new MemoryStorage() as unknown as Storage
globalThis.localStorage = memStore

import { loadProfiles, mergeProfile, saveProfile } from './profiles'
import type { Settings } from './types'

const settings = (over: Partial<Settings>): Settings => ({ mycall: 'KD9TAW', ...over }) as Settings

const MINE: Partial<Settings> = {
  js8CqText: 'CQ DX <MYGRID4>',
  js8ReplyText: 'TNX FER CALL',
  js8SavedMessages: ['GM ES TNX', 'QRV <MYGRID4>'],
  js8SendAtOnce: false,
  js8Pills: false,
  js8WordSuggestions: false,
}

describe('the JS8 compose settings in a config profile', () => {
  beforeEach(() => memStore.clear())

  it('save and load back by value', () => {
    saveProfile('Portable', settings(MINE))
    const loaded = loadProfiles().find((p) => p.name === 'Portable')!
    const merged = mergeProfile(settings({ js8CqText: 'CQ CQ CQ <MYGRID4>', js8SavedMessages: ['TNX 73 GL'], js8SendAtOnce: true }), loaded.settings)
    expect(merged).toMatchObject(MINE)
  })

  it('a profile from before them keeps the current ones', () => {
    saveProfile('Old', settings({}))
    const loaded = loadProfiles().find((p) => p.name === 'Old')!
    const merged = mergeProfile(settings(MINE), loaded.settings)
    expect(merged).toMatchObject(MINE)
  })
})
