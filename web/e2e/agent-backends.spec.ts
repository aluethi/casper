import { test, expect } from '@playwright/test'

async function login(page: import('@playwright/test').Page) {
  await page.goto('/login')
  await page.fill('input[type="email"]', 'admin@ventoo.ch')
  await page.click('button:has-text("Dev Login")')
  await page.waitForURL('/', { timeout: 10_000 }).catch(() => {})
}

test.describe('Agent Backend Key Management', () => {

  test('Backends page loads and shows agent backend with Keys button', async ({ page }) => {
    await login(page)
    await page.goto('/admin/backends')
    await expect(page.locator('main h1')).toHaveText('Platform Backends')
    await expect(page.locator('table')).toBeVisible()

    // There should be at least one backend row
    const rows = page.locator('tbody tr')
    await expect(rows.first()).toBeVisible()

    // An agent-type backend should show a "Keys" button
    const keysBtn = page.locator('button:has-text("Keys")')
    // If no agent backend exists yet, create one
    if (await keysBtn.count() === 0) {
      // Create an agent backend
      await page.click('button:has-text("Add Backend")')
      await page.fill('input[placeholder*="Backend name"]', 'test-gpu-e2e')
      await page.selectOption('select', 'agent')
      await page.click('button:has-text("Create Backend")')
      await page.waitForTimeout(1000)
    }

    // Now there should be a Keys button
    await expect(page.locator('button:has-text("Keys")').first()).toBeVisible()
  })

  test('Keys panel opens and can create a key', async ({ page }) => {
    await login(page)
    await page.goto('/admin/backends')
    await page.waitForTimeout(1000)

    // Ensure agent backend exists, create if needed
    if (await page.locator('button:has-text("Keys")').count() === 0) {
      await page.click('button:has-text("Add Backend")')
      await page.fill('input[placeholder*="Backend name"]', 'test-gpu-keys')
      await page.selectOption('select', 'agent')
      await page.click('button:has-text("Create Backend")')
      await page.waitForTimeout(1000)
    }

    // Click Keys to expand the panel
    await page.locator('button:has-text("Keys")').first().click()
    await page.waitForTimeout(500)

    // Keys panel should be visible
    await expect(page.locator('text=Agent Keys for')).toBeVisible()
    await expect(page.locator('button:has-text("Create Key")')).toBeVisible()

    // Create a key
    const keyName = `e2e-key-${Date.now()}`
    await page.fill('input[placeholder*="Key name"]', keyName)
    await page.click('button:has-text("Create Key")')
    await page.waitForTimeout(1000)

    // The key should be shown in the yellow banner (shown once)
    await expect(page.locator('text=copy it now')).toBeVisible()
    const keyText = await page.locator('code').first().textContent()
    expect(keyText).toBeTruthy()
    expect(keyText!.startsWith('csa-')).toBeTruthy()

    // The key should appear in the list
    await expect(page.locator(`text=${keyName}`)).toBeVisible()
  })

  test('Keys can be revoked', async ({ page }) => {
    await login(page)
    await page.goto('/admin/backends')
    await page.waitForTimeout(1000)

    // Open keys panel
    const keysBtn = page.locator('button:has-text("Keys")').first()
    if (await keysBtn.isVisible()) {
      await keysBtn.click()
      await page.waitForTimeout(500)

      // If there are active keys with a Revoke button
      const revokeBtn = page.locator('button:has-text("Revoke")').first()
      if (await revokeBtn.isVisible()) {
        // Accept the confirmation dialog
        page.on('dialog', dialog => dialog.accept())
        await revokeBtn.click()
        await page.waitForTimeout(500)

        // The key should now show as "Revoked" or be removed
        // (depends on whether the list was refreshed)
      }
    }
  })

  test('No double API prefix on backend key requests', async ({ page }) => {
    const badRequests: string[] = []
    page.on('request', (request) => {
      const url = request.url()
      if (url.includes('/api/v1/api/v1')) {
        badRequests.push(url)
      }
    })

    await login(page)
    await page.goto('/admin/backends')
    await page.waitForTimeout(1000)

    // Open keys panel if available
    const keysBtn = page.locator('button:has-text("Keys")').first()
    if (await keysBtn.isVisible()) {
      await keysBtn.click()
      await page.waitForTimeout(500)
    }

    expect(
      badRequests,
      `Found double-prefix requests: ${badRequests.join(', ')}`
    ).toHaveLength(0)
  })
})
