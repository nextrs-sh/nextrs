import { expect, test } from '@playwright/test';
import { guard } from './guard';

const PAGES = ['/', '/responsive', '/fill', '/placeholder', '/shimmer', '/color', '/background', '/theme'];

for (const path of PAGES) {
  test(`images load on ${path}`, async ({ page }) => {
    const g = guard(page);
    await page.goto(path);
    await page.waitForLoadState('networkidle');
    // Scroll so lazy images are requested.
    await page.evaluate(async () => {
      for (let y = 0; y < document.body.scrollHeight; y += 400) {
        window.scrollTo(0, y);
        await new Promise((r) => setTimeout(r, 50));
      }
    });
    await page.waitForLoadState('networkidle');
    const images = page.locator('img');
    const broken = await images.evaluateAll((els) =>
      els.filter((el) => {
        const img = el as HTMLImageElement;
        // Images from the remote host are not reachable in the test sandbox.
        return img.currentSrc && !img.currentSrc.includes('assets.vercel.com') && !img.currentSrc.includes('url=https') && (!img.complete || img.naturalWidth === 0);
      }).map((el) => (el as HTMLImageElement).currentSrc),
    );
    expect(broken, `broken images: ${broken.join(', ')}`).toEqual([]);
    expect(await images.count()).toBeGreaterThan(0);
    g.assertClean();
  });
}
