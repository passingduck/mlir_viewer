import { expect, test } from '@playwright/test'

// Not a regression test: captures README screenshots from the running app.
// Run with: SCREENSHOT=1 npx playwright test screenshot
test('capture readme screenshots @screenshot', async ({ page }) => {
  test.skip(!process.env.SCREENSHOT, 'screenshot capture is opt-in')
  await page.setViewportSize({ width: 1360, height: 850 })
  await page.goto('/')
  await page.getByText('canonicalize').click()
  await page.getByRole('button', { name: /Diff/ }).click()
  await expect(
    page.locator('.cm-line.diff-removed, .cm-line.diff-added').first(),
  ).toBeVisible()
  await page.screenshot({ path: '../docs/assets/text-diff.png' })

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.getByText('Laying out graph…')).toBeHidden({ timeout: 10_000 })
  const canvas = page.locator('canvas')
  await canvas.focus()
  await canvas.press('ArrowRight')
  await canvas.press('Enter')
  await expect(page.getByRole('heading', { name: /arith/ })).toBeVisible()
  await page.screenshot({ path: '../docs/assets/history.png' })
})
