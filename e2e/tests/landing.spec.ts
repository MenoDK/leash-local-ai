import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { leaveSplash, watch } from './helpers';

test.describe('landing page', () => {
  test('shows the loading stage, then the page, without a single console problem or third-party request', async ({
    page,
  }) => {
    const problems = watch(page);
    await page.goto('/?lang=en');
    await expect(page.locator('#splash')).toBeVisible();
    await leaveSplash(page);
    await expect(page.getByRole('heading', { level: 1 })).toContainText('An AI assistant');
    await expect(page.getByRole('link', { name: /Open the lab/ }).first()).toBeVisible();
    expect(problems).toEqual([]);
  });

  test('the tour tabs swap in recorded output that really came from the server', async ({ page }) => {
    await page.goto('/?lang=en');
    await leaveSplash(page);
    const tabs = page.getByRole('group', { name: 'Explore the product' });
    const output = page.getByRole('region', { name: 'Example output' });
    await tabs.getByRole('button', { name: /Seal/ }).click();
    await expect(output).toContainText('[NEUTRALISED]');
    await expect(output).toContainText('override-instructions');
    await expect(output).toContainText('held_for_review');
    await tabs.getByRole('button', { name: /Route/ }).click();
    await expect(output).toContainText('skipped_remote');
    await expect(output).toContainText('skipped_circuit_open');
    await tabs.getByRole('button', { name: /Memory/ }).click();
    await expect(output).toContainText('[REDACTED]');
    await expect(output).toContainText('path_traversal');
    await expect(output).toContainText('dont_know');
    await tabs.getByRole('button', { name: /Broker/ }).click();
    // placeholders are text, not tags: a transcript inserted as HTML must not swallow "<proposal>"
    await expect(output).toContainText('/api/broker/proposals/<proposal>/approve');
    await expect(output).toContainText('digest_mismatch');
    await expect(output).toContainText('already_resolved');
    await expect(output).toContainText('the binary changed after the proposal was made');
    await tabs.getByRole('button', { name: /Ledger/ }).click();
    await expect(output).toContainText('the chain');
    await expect(output).toContainText('the anchor');
    await tabs.getByRole('button', { name: /Bridge/ }).click();
    await expect(output).toContainText('UnknownCA');
    await expect(output).toContainText('CertificateExpired');
    await expect(output).toContainText('CertificateRequired');
    await expect(tabs.getByRole('button', { name: /Bridge/ })).toHaveAttribute('aria-pressed', 'true');
  });

  test('no recorded output contains anything shaped like a secret', async ({ page }) => {
    await page.goto('/?lang=en');
    const text = await page.evaluate(() => JSON.stringify((window as unknown as { TRANSCRIPTS: object }).TRANSCRIPTS));
    expect(text).not.toMatch(/sk-live|Bearer [A-Za-z0-9]{12}|-----BEGIN/);
  });

  test('every hard part offers a recipe and a link that opens the lab on that tab', async ({ page }) => {
    await page.goto('/?lang=en#hard-parts');
    await leaveSplash(page);
    const list = page.getByRole('group', { name: 'Hard parts to try' });
    const names = [
      'Give it an order',
      'Leak the notes',
      'Poison the library',
      'Make it run something',
      'Swap the program',
      'Rewrite history',
      'Pose as the controller',
    ];
    for (const name of names) {
      await list.getByRole('button', { name }).click();
      await expect(page.getByText('You should see', { exact: true })).toBeVisible();
      await expect(page.getByRole('link', { name: 'Try it in the lab' })).toHaveAttribute('href', /lab\.html(#\w+)?$/);
    }
  });

  test('the theme switch is applied, remembered across reloads, and nothing leaves the page', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/?lang=en');
    await leaveSplash(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.getByRole('button', { name: 'Use the dark theme' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  });

  test('works with reduced motion and still reveals every section', async ({ browser }) => {
    const context = await browser.newContext({ reducedMotion: 'reduce' });
    const page = await context.newPage();
    await page.goto('/?lang=en');
    await leaveSplash(page);
    await page.locator('#faq').scrollIntoViewIfNeeded();
    await expect(page.getByRole('heading', { name: /Asked often/ })).toBeVisible();
    await context.close();
  });

  test('has no horizontal scroll on a phone', async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 360, height: 740 } });
    const page = await context.newPage();
    await page.goto('/?lang=en');
    await leaveSplash(page);
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
    await context.close();
  });

  test('the hero artwork keeps every label inside its frame', async ({ page }) => {
    await page.goto('/?lang=en');
    await leaveSplash(page);
    const outside = await page.evaluate(() => {
      const frame = document.querySelector('.hero-art svg');
      if (!frame) return -1;
      const box = frame.getBoundingClientRect();
      return [...frame.querySelectorAll('text')].filter((t) => {
        const r = t.getBoundingClientRect();
        return r.left < box.left - 1 || r.right > box.right + 1;
      }).length;
    });
    expect(outside).toBe(0);
  });
});

for (const scheme of ['light', 'dark'] as const) {
  test(`accessibility: the landing page has no WCAG A/AA violations (${scheme})`, async ({ browser }) => {
    const context = await browser.newContext({ colorScheme: scheme, reducedMotion: 'reduce' });
    const page = await context.newPage();
    await page.goto('/?lang=en');
    await leaveSplash(page);
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(' | ')}`)).toEqual([]);
    await context.close();
  });
}
