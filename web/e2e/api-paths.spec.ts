import { test, expect } from '@playwright/test'

/**
 * This test catches the double-prefix bug where API calls go to
 * /api/v1/api/v1/... instead of /api/v1/...
 *
 * It intercepts all network requests during navigation and flags
 * any that contain a duplicated path segment.
 */
test.describe('API path integrity', () => {

  test('no requests contain double /api/v1/api/v1 prefix', async ({ page }) => {
    const badRequests: string[] = []

    // Intercept all requests and check for double prefix
    page.on('request', (request) => {
      const url = request.url()
      if (url.includes('/api/v1/api/v1')) {
        badRequests.push(url)
      }
      // Also catch /api/v1/api/ without the second v1
      if (/\/api\/v1\/api\//.test(url)) {
        badRequests.push(url)
      }
    })

    // Login first
    await page.goto('/login')
    await page.fill('input[type="email"]', 'admin@ventoo.ch')
    await page.click('button:has-text("Dev Login")')

    // Wait for redirect to dashboard
    await page.waitForURL('/', { timeout: 10_000 }).catch(() => {})

    // Visit every main page and let API calls fire
    const pages = [
      '/catalog',
      '/deployments',
      '/keys',
      '/agents',
      '/knowledge',
      '/memory',
      '/conversations',
      '/usage',
      '/audit',
      '/admin/users',
      '/admin/secrets',
      '/admin/models',
      '/admin/backends',
      '/admin/quotas',
      '/admin/tenants',
    ]

    for (const path of pages) {
      await page.goto(path)
      // Give API calls time to fire
      await page.waitForTimeout(500)
    }

    // Assert no double-prefix requests were made
    expect(
      badRequests,
      `Found ${badRequests.length} request(s) with double /api/v1 prefix:\n${badRequests.join('\n')}`
    ).toHaveLength(0)
  })

  test('all API requests use correct prefixes', async ({ page }) => {
    const apiRequests: string[] = []

    page.on('request', (request) => {
      const url = new URL(request.url())
      const path = url.pathname
      // Collect requests that look like API calls (not static assets)
      if (
        path.startsWith('/api/') ||
        path.startsWith('/auth/') ||
        path.startsWith('/v1/') ||
        path.startsWith('/health') ||
        path.startsWith('/metrics')
      ) {
        apiRequests.push(path)
      }
    })

    await page.goto('/login')
    await page.fill('input[type="email"]', 'admin@ventoo.ch')
    await page.click('button:has-text("Dev Login")')
    await page.waitForURL('/', { timeout: 10_000 }).catch(() => {})

    // Visit a few key pages
    await page.goto('/agents')
    await page.waitForTimeout(500)
    await page.goto('/catalog')
    await page.waitForTimeout(500)

    // Every /api/ request should start with /api/v1/
    const badApiPaths = apiRequests.filter(
      (p) => p.startsWith('/api/') && !p.startsWith('/api/v1/')
    )

    expect(
      badApiPaths,
      `Found API requests with wrong prefix:\n${badApiPaths.join('\n')}`
    ).toHaveLength(0)
  })
})
