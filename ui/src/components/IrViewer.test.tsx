import { render, screen } from '@testing-library/react'
import { expect, test } from 'vitest'
import type { IrPage } from '../api'
import { IrViewer } from './IrViewer'

const before: IrPage = {
  pass_id: 2,
  side: 'before',
  text: 'module { %0 = arith.constant 0 : i32 }',
  offset: 0,
  next_offset: null,
  total_bytes: 38,
}

test('creates a read-only editor and explains a missing side', () => {
  const { container } = render(<IrViewer before={before} after={null} />)
  expect(screen.getByRole('heading', { name: 'Before' })).toBeInTheDocument()
  expect(screen.getByRole('heading', { name: 'After' })).toBeInTheDocument()
  expect(screen.getByText('No after snapshot')).toBeInTheDocument()
  expect(container.querySelectorAll('.cm-editor')).toHaveLength(1)
})
