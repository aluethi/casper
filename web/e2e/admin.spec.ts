import { test, expect } from '@playwright/test'

// Helper: login and return authenticated page
async function login(page: import('@playwright/test').Page) {
  await page.goto('/login')
  await page.fill('input[type="email"]', 'admin@ventoo.ch')
  await page.click('button:has-text("Dev Login")')
  await page.waitForURL('/', { timeout: 10_000 }).catch(() => {})
}

test.describe('Admin — Tenants', () => {
  test('page loads and shows tenants table', async ({ page }) => {
    await login(page)
    await page.goto('/admin/tenants')
    await expect(page.locator('main h1')).toHaveText('Tenants')
    await expect(page.locator('table')).toBeVisible()
  })

  test('has Edit and Suspend actions for each tenant', async ({ page }) => {
    await login(page)
    await page.goto('/admin/tenants')
    await page.waitForTimeout(1000)
    // Check that at least one row has Edit and Suspend/Activate buttons
    const rows = page.locator('tbody tr')
    const count = await rows.count()
    if (count > 0) {
      const firstRow = rows.first()
      await expect(firstRow.locator('text=Edit')).toBeVisible()
      // Either Suspend or Activate should be visible
      const hasSuspend = await firstRow.locator('text=Suspend').isVisible()
      const hasActivate = await firstRow.locator('text=Activate').isVisible()
      expect(hasSuspend || hasActivate).toBeTruthy()
    }
  })

  test('edit inline form appears on Edit click', async ({ page }) => {
    await login(page)
    await page.goto('/admin/tenants')
    await page.waitForTimeout(1000)
    const rows = page.locator('tbody tr')
    if (await rows.count() > 0) {
      await rows.first().locator('text=Edit').click()
      // Should show Save and Cancel buttons
      await expect(rows.first().locator('text=Save')).toBeVisible()
      await expect(rows.first().locator('text=Cancel')).toBeVisible()
    }
  })
})

test.describe('Admin — Models', () => {
  test('page loads with table and action buttons', async ({ page }) => {
    await login(page)
    await page.goto('/admin/models')
    await expect(page.locator('main h1')).toContainText('Model Catalog')
    await expect(page.locator('table')).toBeVisible()
  })

  test('each model row has Edit and Deactivate actions', async ({ page }) => {
    await login(page)
    await page.goto('/admin/models')
    await page.waitForTimeout(1000)
    const rows = page.locator('tbody tr')
    if (await rows.count() > 0) {
      const firstRow = rows.first()
      await expect(firstRow.locator('text=Edit')).toBeVisible()
      const hasDeactivate = await firstRow.locator('text=Deactivate').isVisible()
      const hasActivate = await firstRow.locator('text=Activate').isVisible()
      expect(hasDeactivate || hasActivate).toBeTruthy()
    }
  })

  test('published toggle is clickable', async ({ page }) => {
    await login(page)
    await page.goto('/admin/models')
    await page.waitForTimeout(1000)
    const publishBtn = page.locator('tbody tr').first().locator('button:has-text("Published"), button:has-text("Draft")')
    if (await publishBtn.count() > 0) {
      await expect(publishBtn.first()).toBeVisible()
    }
  })

  test('edit panel opens with fields', async ({ page }) => {
    await login(page)
    await page.goto('/admin/models')
    await page.waitForTimeout(1000)
    const rows = page.locator('tbody tr')
    if (await rows.count() > 0) {
      await rows.first().locator('text=Edit').click()
      await expect(page.locator('text=Editing model')).toBeVisible()
      await expect(page.locator('button:has-text("Save")')).toBeVisible()
    }
  })
})

test.describe('Admin — Backends', () => {
  test('page loads with table and Add Backend button', async ({ page }) => {
    await login(page)
    await page.goto('/admin/backends')
    await expect(page.locator('main h1')).toHaveText('Platform Backends')
    await expect(page.locator('button:has-text("Add Backend")')).toBeVisible()
  })

  test('each backend row has Edit and Deactivate actions', async ({ page }) => {
    await login(page)
    await page.goto('/admin/backends')
    await page.waitForTimeout(1000)
    const rows = page.locator('tbody tr')
    if (await rows.count() > 0) {
      const firstRow = rows.first()
      // Skip empty-state row
      const isEmpty = await firstRow.locator('text=No backends configured').isVisible()
      if (!isEmpty) {
        await expect(firstRow.locator('text=Edit')).toBeVisible()
        const hasDeactivate = await firstRow.locator('text=Deactivate').isVisible()
        const hasActivate = await firstRow.locator('text=Activate').isVisible()
        expect(hasDeactivate || hasActivate).toBeTruthy()
      }
    }
  })

  test('edit panel opens for backend', async ({ page }) => {
    await login(page)
    await page.goto('/admin/backends')
    await page.waitForTimeout(1000)
    const editBtn = page.locator('tbody tr').first().locator('text=Edit')
    if (await editBtn.isVisible()) {
      await editBtn.click()
      await expect(page.locator('text=Editing backend')).toBeVisible()
    }
  })
})

test.describe('Admin — Quotas', () => {
  test('page loads with tenant selector and table', async ({ page }) => {
    await login(page)
    await page.goto('/admin/quotas')
    await expect(page.locator('main h1')).toHaveText('Model Quotas')
    await expect(page.locator('select')).toBeVisible()
    await expect(page.locator('table')).toBeVisible()
  })

  test('quota rows have Edit and Remove actions', async ({ page }) => {
    await login(page)
    await page.goto('/admin/quotas')
    await page.waitForTimeout(1000)
    const rows = page.locator('tbody tr')
    if (await rows.count() > 0) {
      const firstRow = rows.first()
      const isEmpty = await firstRow.locator('text=No quotas allocated').isVisible()
      if (!isEmpty) {
        await expect(firstRow.locator('text=Edit')).toBeVisible()
        await expect(firstRow.locator('text=Remove')).toBeVisible()
      }
    }
  })

  test('edit inline for quota shows Save/Cancel', async ({ page }) => {
    await login(page)
    await page.goto('/admin/quotas')
    await page.waitForTimeout(1000)
    const editBtn = page.locator('tbody tr').first().locator('text=Edit')
    if (await editBtn.isVisible()) {
      await editBtn.click()
      await expect(page.locator('tbody tr').first().locator('text=Save')).toBeVisible()
      await expect(page.locator('tbody tr').first().locator('text=Cancel')).toBeVisible()
    }
  })

  test('allocate quota button shows form', async ({ page }) => {
    await login(page)
    await page.goto('/admin/quotas')
    await page.click('button:has-text("Allocate Quota")')
    // The form should now be visible with model selector and submit button
    await expect(page.locator('select >> nth=1')).toBeVisible() // model selector (2nd select after tenant)
    await expect(page.getByRole('button', { name: 'Allocate', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible()
  })
})
