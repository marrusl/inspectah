/**
 * API endpoint tests for the refine server.
 * Validates REST API responses independently of the UI.
 */
import { test, expect } from '@playwright/test';
import { resetServer } from './helpers';

test.describe('Refine API endpoints', () => {
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });

  test('GET /api/health returns status and re_render flag', async ({ request }) => {
    const resp = await request.get('/api/health');
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body).toEqual({
      status: 'ok',
      re_render: true,
    });
  });

  test('GET /api/snapshot returns snapshot and revision', async ({ request }) => {
    const resp = await request.get('/api/snapshot');
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();

    expect(body.snapshot).toBeDefined();
    expect(typeof body.revision).toBe('number');
    expect(body.revision).toBeGreaterThanOrEqual(1);

    // Snapshot should have meta and section keys
    const snap = body.snapshot;
    expect(snap.meta).toBeDefined();
    expect(snap.meta.hostname).toBeDefined();
  });

  test('GET / returns HTML with correct content-type', async ({ request }) => {
    const resp = await request.get('/');
    expect(resp.ok()).toBeTruthy();
    const contentType = resp.headers()['content-type'];
    expect(contentType).toContain('text/html');
  });

  test('GET /nonexistent returns 404', async ({ request }) => {
    const resp = await request.get('/nonexistent-path-xyz');
    expect(resp.status()).toBe(404);
  });

  test('POST /api/render accepts valid snapshot with 200', async ({ request }) => {
    const snapResp = await request.get('/api/snapshot');
    const snapBody = await snapResp.json();

    const renderResp = await request.post('/api/render', {
      data: { snapshot: snapBody.snapshot },
    });

    expect(renderResp.status()).toBe(200);
    const body = await renderResp.json();
    expect(body.html).toBeDefined();
    expect(body.containerfile).toBeDefined();
    expect(body.containerfile).toContain('FROM');
    expect(body.render_id).toBeDefined();
    expect(body.revision).toBeGreaterThan(0);
    expect(body.triage_manifest).toBeDefined();
  });

  test('POST /api/render rejects malformed payload with 400', async ({ request }) => {
    const snapBefore = await request.get('/api/snapshot');
    const revisionBefore = (await snapBefore.json()).revision;

    const renderResp = await request.post('/api/render', {
      data: { snapshot: { not_valid: true } },
    });

    expect(renderResp.status()).toBe(400);
    const body = await renderResp.json();
    expect(body.error).toBeDefined();
    expect(typeof body.error).toBe('string');

    const snapAfter = await request.get('/api/snapshot');
    const revisionAfter = (await snapAfter.json()).revision;
    expect(revisionAfter).toBe(revisionBefore);
  });

  test('GET /api/tarball returns gzip when render_id is current', async ({ request }) => {
    // The tarball endpoint serves the current output directory.
    // Without a render_id, it should still return data.
    const tarballResp = await request.get('/api/tarball');
    expect(tarballResp.ok()).toBeTruthy();
    expect(tarballResp.headers()['content-type']).toContain('application/gzip');
    expect(tarballResp.headers()['content-disposition']).toContain('attachment');
  });
});

test.describe('Refine API - single host', () => {
  test('single-host server health check', async ({ request }) => {
    const url = process.env.REFINE_SINGLE_URL;
    if (!url) {
      test.skip(true, 'REFINE_SINGLE_URL not set');
      return;
    }

    const resp = await request.get(`${url}/api/health`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.status).toBe('ok');
  });

  test('single-host snapshot has expected structure', async ({ request }) => {
    const url = process.env.REFINE_SINGLE_URL;
    if (!url) {
      test.skip(true, 'REFINE_SINGLE_URL not set');
      return;
    }

    const resp = await request.get(`${url}/api/snapshot`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.snapshot.meta).toBeDefined();
  });
});
