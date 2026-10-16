import { expect, it } from 'vitest'
import { latestOnly } from './latestOnly'

const deferred = () => {
  let resolve!: () => void, reject!: (error: unknown) => void
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const settle = () => new Promise(resolve => setTimeout(resolve, 0))

it('keeps one send in flight and, when it settles, sends only the newest value moved to meanwhile', async () => {
  const sent: number[] = [], pending: ReturnType<typeof deferred>[] = []
  const send = latestOnly((value: number) => { sent.push(value); const d = deferred(); pending.push(d); return d.promise }, () => {})
  send(1); send(2); send(3); send(4)
  expect(sent).toEqual([1])
  pending[0].resolve(); await settle()
  expect(sent).toEqual([1, 4])
  pending[1].resolve(); await settle()
  expect(sent).toEqual([1, 4])
  send(5)
  expect(sent).toEqual([1, 4, 5])
})

it('reports each failure once and still sends the value queued behind it', async () => {
  const errors: unknown[] = [], sent: number[] = [], pending: ReturnType<typeof deferred>[] = []
  const send = latestOnly((value: number) => { sent.push(value); const d = deferred(); pending.push(d); return d.promise }, error => errors.push(error))
  send(1); send(2)
  pending[0].reject('busy'); await settle()
  expect(errors).toEqual(['busy'])
  expect(sent).toEqual([1, 2])
})
