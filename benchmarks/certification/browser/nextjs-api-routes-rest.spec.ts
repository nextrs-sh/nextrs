import { expect, test } from '@playwright/test';
import { guard } from './guard';

test('client-side SWR fetches users and a user from the API routes', async ({ page }) => {
  const g = guard(page);
  await page.goto('/');
  const links = page.locator('a[href^="/user/"]');
  await expect(links).toHaveCount(3);
  await links.nth(1).click();
  await expect(page).toHaveURL(/\/user\/2$/);
  await expect(page.locator('body')).toContainText('User 2');
  g.assertClean();
});
