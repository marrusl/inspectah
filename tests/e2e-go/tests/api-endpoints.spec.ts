/**
 * API endpoint tests for the refine server.
 * Validates REST API responses independently of the UI.
 */
import { test, expect } from '@playwright/test';

test.describe('Refine API endpoints', () => {
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
    // The refine server handles non-root paths as static file lookups
    // or returns 404
    expect(resp.status()).toBeGreaterThanOrEqual(400);
  });

  test('POST /api/render accepts snapshot payload', async ({ request }) => {
    // First get the current snapshot
    const snapResp = await request.get('/api/snapshot');
    const { snapshot, revision } = await snapResp.json();

    // Post it back to trigger a re-render
    const renderResp = await request.post('/api/render', {
      data: { snapshot, revision },
    });
    expect(renderResp.ok()).toBeTruthy();

    const body = await renderResp.json();
    expect(body.html).toBeDefined();
    expect(body.snapshot).toBeDefined();
    expect(body.containerfile).toBeDefined();
    expect(body.triage_manifest).toBeDefined();
    expect(body.render_id).toBeDefined();
    expect(body.revision).toBeGreaterThan(0);
  });

  test('GET /api/tarball returns gzip data', async ({ request }) => {
    // First do a render to get a render_id
    const snapResp = await request.get('/api/snapshot');
    const { snapshot, revision } = await snapResp.json();

    const renderResp = await request.post('/api/render', {
      data: { snapshot, revision },
    });
    const { render_id } = await renderResp.json();

    // Download tarball with the render_id
    const tarballResp = await request.get(`/api/tarball?render_id=${render_id}`);
    expect(tarballResp.ok()).toBeTruthy();
    expect(tarballResp.headers()['content-type']).toContain('application/gzip');
    expect(tarballResp.headers()['content-disposition']).toContain('attachment');
  });
});

test.describe('Refine API - single host', () => {
  test('single-host server health check', async ({ request }) => {
    const url = process.env.REFINE_SINGLE_URL;
    if (!url) {
      test.skip();
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
      test.skip();
      return;
    }

    const resp = await request.get(`${url}/api/snapshot`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.snapshot.meta).toBeDefined();
  });
});
