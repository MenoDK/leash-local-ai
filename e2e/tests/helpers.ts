import { type APIRequestContext, type Page, expect } from '@playwright/test';

const BASE = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:8090');

/** Collects what would make a reviewer frown: console errors, page errors (CSP violations land here) and third-party calls. */
export function watch(page: Page, { allowStatus = [] as number[] } = {}): string[] {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() !== 'error' && m.type() !== 'warning') return;
    // Chromium logs every non-2xx fetch as an error. The lab provokes some on purpose; allow only the listed ones.
    const status = /status of (\d{3})/.exec(m.text());
    if (status && allowStatus.includes(Number(status[1]))) return;
    problems.push(`console ${m.type()}: ${m.text()}`);
  });
  page.on('request', (r) => {
    const url = new URL(r.url());
    if (url.protocol !== 'data:' && url.protocol !== 'blob:' && url.host !== BASE.host) {
      problems.push(`third-party request: ${r.url()}`);
    }
  });
  return problems;
}

export async function leaveSplash(page: Page): Promise<void> {
  await expect(page.locator('#splash')).toHaveCount(0, { timeout: 10_000 });
}

/** Open the lab on a tab and wait until it is there. There is no sign-in: the lab is a local tool. */
export async function openLab(page: Page, name: string): Promise<void> {
  await page.goto(`/lab.html#${name.toLowerCase()}`);
  await leaveSplash(page);
  await expect(page.getByRole('tab', { name })).toHaveAttribute('aria-selected', 'true');
}

export const tab = (page: Page, name: string) => page.getByRole('tab', { name });

/** The tab panel that is open. Panels that were visited and left stay in the page, hidden, so every locator starts here. */
export const active = (page: Page) => page.locator('[role="tabpanel"]:not([hidden])');

/** The answer and result cards in the open tab, newest first. */
export const cards = (page: Page) => active(page).locator('.box.arrive');

/** Put the server back to a known state: both models healthy, circuits closed, no proposal waiting, agent dormant. */
export async function resetServer(request: APIRequestContext): Promise<void> {
  for (const id of ['local-stand-in', 'remote-stand-in']) {
    const res = await request.post(`/api/lab/gateway/provider/${id}`, { data: { behaviour: 'healthy' } });
    expect(res.ok()).toBeTruthy();
  }
  const waiting = (await (await request.get('/api/broker/proposals')).json()) as { proposals: { id: string; state: string }[] };
  for (const p of waiting.proposals.filter((x) => x.state === 'pending')) {
    await request.post(`/api/broker/proposals/${p.id}/deny`, { data: {} });
  }
  await request.post('/api/bridge/deactivate', { data: {} });
  await request.post('/api/bridge/reconnect', { data: {} });
  // A circuit opened by an earlier test closes after its cooldown, on the first call that succeeds.
  for (let i = 0; i < 40; i += 1) {
    const res = await request.post('/api/gateway/complete', { data: { text: 'settle', provenance: 'document' } });
    const body = (await res.json()) as { model?: string };
    if (body.model === 'stand-in:local-stand-in') return;
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error('the local circuit did not close within 40 seconds');
}
