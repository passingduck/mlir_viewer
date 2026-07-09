import { expect, test } from '@playwright/test'

test('text and graph views with diff toggles', async ({ page }) => {
  const consoleErrors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  await page.goto('/')
  await page.getByText('canonicalize').click()

  await page.getByRole('button', { name: /Diff/ }).click()
  await expect(page.locator('.cm-line.diff-removed, .cm-line.diff-added').first()).toBeVisible()

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas')).toBeVisible()
  await expect(page.locator('.graph-legend .chip.added')).toBeVisible()
  await expect(page.getByText('Laying out graph…')).toBeHidden({ timeout: 10_000 })

  const canvas = page.locator('canvas')
  await canvas.focus()
  await canvas.press('ArrowRight')
  await canvas.press('Enter')
  await expect(page.getByRole('tab', { name: 'History' })).toBeVisible()
  await page.getByRole('tab', { name: 'History' }).click()
  await expect(page.getByText('AddIToShift')).toBeVisible()
  await expect(page.getByText('listener').first()).toBeVisible()
  await page.getByRole('button', { name: 'View IR' }).first().click()
  await expect(page.locator('.editor-grid')).toBeVisible()

  await page.getByRole('button', { name: 'Text' }).click()
  await expect(page.locator('.editor-grid')).toBeVisible()
  expect(consoleErrors).toEqual([])
})
