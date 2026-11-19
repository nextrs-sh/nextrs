import { expect, test } from '@playwright/test';
import { guard } from './guard';

test('anonymous visitors are sent to log in and see no notes', async ({ page }) => {
  const g = guard(page);
  await page.goto('/');
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', { name: 'Log in' })).toBeVisible();
  await expect(page.getByText('Finish the analytical engine notes.')).toHaveCount(0);
  g.assertClean();
});

test('wrong credentials are refused without a session', async ({ page, context }) => {
  const g = guard(page);
  await page.goto('/login');
  await page.getByLabel('Password').fill('wrong');
  await page.getByRole('button', { name: 'Log in' }).click();
  await expect(page.getByText('These credentials do not match our records.')).toBeVisible();
  expect((await context.cookies()).find((c) => c.name === 'app_session')).toBeUndefined();
  g.assertClean();
});

test('log in, read and add notes, navigate, log out', async ({ page, context }) => {
  const g = guard(page);
  await page.goto('/login');
  await page.getByRole('button', { name: 'Log in' }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByText('Ada Lovelace')).toBeVisible();
  await expect(page.getByText('Cache Components is now enabled.')).toBeVisible();
  await expect(page.getByRole('link', { name: 'Finish the analytical engine notes.' })).toBeVisible();
  const session = (await context.cookies()).find((c) => c.name === 'app_session');
  expect(session?.httpOnly).toBe(true);

  const note = `Browser note ${Date.now()}`;
  await page.getByPlaceholder('Add a note').fill(note);
  await page.getByRole('button', { name: 'Add' }).click();
  await expect(page.getByRole('link', { name: note })).toBeVisible();

  await page.getByRole('link', { name: 'Finish the analytical engine notes.' }).click();
  await expect(page).toHaveURL(/\/notes\/n1$/);
  await expect(page.getByText('Finish the analytical engine notes.').first()).toBeVisible();
  await page.goBack();

  await page.getByRole('button', { name: 'Log out' }).click();
  await expect(page).toHaveURL(/\/login$/);
  expect((await context.cookies()).find((c) => c.name === 'app_session')).toBeUndefined();
  await page.goto('/notes/n1');
  await expect(page.getByText('Finish the analytical engine notes.')).toHaveCount(0);
  g.assertClean();
});
