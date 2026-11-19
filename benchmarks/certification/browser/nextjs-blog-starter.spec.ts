import { expect, test } from '@playwright/test';
import { guard } from './guard';

test('home lists posts and a post renders with its cover image', async ({ page }) => {
  const g = guard(page);
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Blog.' })).toBeVisible();
  await page.getByRole('link', { name: 'Dynamic Routing and Static Generation' }).first().click();
  await expect(page).toHaveURL(/\/posts\/dynamic-routing$/);
  await expect(page.getByRole('heading', { name: 'Dynamic Routing and Static Generation' })).toBeVisible();
  const cover = page.locator('img').first();
  await expect(cover).toBeVisible();
  expect(await cover.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
  await page.goBack();
  await expect(page.getByRole('heading', { name: 'Blog.' })).toBeVisible();
  g.assertClean();
});
