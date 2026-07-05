import { expect, test } from '@playwright/test'

test('text and graph views with diff toggles', async ({ page }) => {
  await page.goto('/')
  await page.getByText('canonicalize').click()

  await page.getByRole('button', { name: /Diff/ }).click()
  await expect(page.locator('.cm-line.diff-removed, .cm-line.diff-added').first()).toBeVisible()

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas')).toBeVisible()
  await expect(page.getByText('Laying out graph…')).toBeHidden({ timeout: 10_000 })
  await expect(page.locator('.graph-legend .chip.added')).toBeVisible()

  await page.getByRole('button', { name: 'Text' }).click()
  await expect(page.locator('.editor-grid')).toBeVisible()
})
