import { expect, test } from '@playwright/test'

test('inspector, palette search, and layout persistence', async ({ page }) => {
  const consoleErrors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  await page.goto('/')
  await page.getByText('canonicalize').click()

  // Select an op from the text view -> inspector opens on structure
  await page.locator('.cm-line.selectable-op').first().click()
  await expect(page.getByRole('tab', { name: 'Structure' })).toBeVisible()
  await expect(page.locator('.op-structure')).toBeVisible()

  // History tab shows the provenance chain
  await page.getByRole('tab', { name: 'History' }).click()
  await expect(
    page.getByText(/replaced|unchanged|modified|disappeared/).first(),
  ).toBeVisible()

  // Palette: search an op pipeline-wide and jump to it
  await page.keyboard.press('ControlOrMeta+k')
  await page.getByPlaceholder('Search passes, functions, ops…').fill('shli')
  await page.locator('[cmdk-item]', { hasText: /arith\.shli/ }).first().click()
  await page.getByRole('tab', { name: 'Structure' }).click()
  await expect(page.locator('.op-structure')).toContainText('arith.shli')

  // Pass stepping
  await page.keyboard.press('Escape')
  await page.keyboard.press(']')
  await expect(page.getByRole('button', { name: /dce/ })).toHaveAttribute('aria-current', 'true')

  // Layout persistence across reload
  const saved = await page.evaluate(() => localStorage.getItem('mlir-viewer-layout-v1'))
  expect(saved).toBeTruthy()
  await page.reload()
  await expect(page.getByText('canonicalize')).toBeVisible()

  expect(consoleErrors).toEqual([])
})
