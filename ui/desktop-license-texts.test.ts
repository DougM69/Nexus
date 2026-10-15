// The desktop build's license texts, which Settings ▸ Licenses reads (src/components/SettingsLicenses.tsx):
// NOTICE.txt, the repository's NOTICE as it is, then THIRD-PARTY.txt and rust/THIRD-PARTY.txt. Runs
// the plugin vite.config.ts builds with, as the build runs it, so a NOTICE dropped from the build or
// changed on its way is a red here and not a dialog that says the texts are not in this build.
//
// Beside vite.config.ts rather than under src/: the app's TypeScript project (tsconfig.app.json)
// covers src/ alone, and the config and remote-licenses.ts are the node project's.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { Plugin } from 'vite'
import config from './vite.config'

describe('the desktop build emits the texts Settings ▸ Licenses shows', () => {
  it('emits the repository NOTICE as NOTICE.txt, as it is, beside the two third-party files', () => {
    const plugins = ((config as { plugins?: unknown[] }).plugins ?? []).flat(Infinity) as Plugin[]
    const plugin = plugins.find((p) => p?.name === 'desktop-license-texts')
    expect(plugin, 'the plugin is in the build').toBeTruthy()
    // A bundle as the build hands it over: a chunk holding React, which the license inventory
    // requires of every desktop bundle (remote-licenses.ts).
    const react = fileURLToPath(new URL('./node_modules/react/index.js', import.meta.url))
    const bundle = { 'index.js': { type: 'chunk', modules: { [react]: {} } } }
    const emitted: Record<string, string> = {}
    const generate = plugin!.generateBundle as (this: unknown, options: unknown, bundle: unknown) => void
    generate.call({ emitFile: (file: { fileName: string; source: string }) => { emitted[file.fileName] = file.source } }, {}, bundle)
    expect(Object.keys(emitted).sort()).toEqual(['NOTICE.txt', 'THIRD-PARTY.txt', 'rust/THIRD-PARTY.txt'])
    expect(emitted['NOTICE.txt']).toBe(readFileSync(new URL('../NOTICE', import.meta.url), 'utf8'))
  })
})
