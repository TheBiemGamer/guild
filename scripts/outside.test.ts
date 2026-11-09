import { test } from 'node:test'
import assert from 'node:assert/strict'
import { keepOnBackForward } from '../src/lib/outside.ts'

const press = (button: number) => {
  let prevented = false
  keepOnBackForward({ button, preventDefault: () => (prevented = true) })
  return prevented
}

test('back and forward buttons never close a popover', () => {
  assert.equal(press(3), true)
  assert.equal(press(4), true)
})

test('left, middle and right clicks outside still close it', () => {
  assert.equal(press(0), false)
  assert.equal(press(1), false)
  assert.equal(press(2), false)
})
