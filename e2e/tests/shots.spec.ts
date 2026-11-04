import { type Page, test } from '@playwright/test';
import { leaveSplash, openLab, resetServer } from './helpers';

// Only with SHOTS=1 (`make screenshots`): these are the pictures in the README, taken from the real server.
test.skip(process.env.SHOTS !== '1', 'screenshots are taken on request');
test.use({ viewport: { width: 1360, height: 860 }, colorScheme: 'light', contextOptions: { reducedMotion: 'reduce' } });

const OUT = process.env.SHOTS_DIR ?? '../docs/img';
const settle = async (page: Page) => {
  await page.evaluate(() => window.scrollTo(0, 0)); // clicking scrolled the page; the picture starts at the top
  await page.waitForTimeout(5000); // let the confirmation toast leave
};

test('landing', async ({ page }) => {
  await page.goto('/?lang=en');
  await leaveSplash(page);
  await page.screenshot({ path: `${OUT}/landing.png` });
});

test('lab: private text, a failing local model, and what the gateway did', async ({ page, request }) => {
  await resetServer(request);
  await openLab(page, 'Route');
  await page.getByRole('button', { name: 'failing' }).first().click();
  await page.getByRole('button', { name: 'Send a public sentence' }).click();
  await page.getByRole('button', { name: 'Send private notes' }).click();
  await page.setViewportSize({ width: 1360, height: 1200 });
  await settle(page);
  await page.screenshot({ path: `${OUT}/lab-route.png` });
  await resetServer(request);
});

test('lab: the broker refuses, waits for a person, and catches a swapped program', async ({ page }) => {
  await openLab(page, 'Broker');
  await page.getByRole('button', { name: 'say -rf' }).click();
  await page.getByRole('button', { name: 'say hello' }).click();
  await page.getByRole('button', { name: 'Change the file after I approve' }).click();
  await page.setViewportSize({ width: 1360, height: 1700 });
  await settle(page);
  await page.screenshot({ path: `${OUT}/lab-broker.png` });
});

test('lab: forged copies of the ledger and who caught them', async ({ page }) => {
  await openLab(page, 'Ledger');
  await page.getByRole('button', { name: 'Edit an entry' }).click();
  await page.getByRole('button', { name: 'Cut the end off' }).click();
  await page.setViewportSize({ width: 1360, height: 1300 });
  await settle(page);
  await page.screenshot({ path: `${OUT}/lab-ledger.png` });
});
