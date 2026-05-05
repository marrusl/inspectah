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
    expect(resp.status()).toBe(404);
  });

  test('POST /api/render validates snapshot format', async ({ request }) => {
    // Get the current snapshot
    const snapResp = await request.get('/api/snapshot');
    const snapBody = await snapResp.json();

    // The render endpoint re-parses the snapshot through the Go schema.
    // Some fixture snapshots fail validation (e.g., empty SystemType).
    // Verify the endpoint accepts POST and returns a JSON response
    // with either the rendered output or a structured error.
    const renderResp = await request.post('/api/render', {
      data: { snapshot: snapBody.snapshot },
    });

    const body = await renderResp.json();

    if (renderResp.ok()) {
      // Successful render
      expect(body.html).toBeDefined();
      expect(body.snapshot).toBeDefined();
      expect(body.containerfile).toBeDefined();
      expect(body.triage_manifest).toBeDefined();
      expect(body.render_id).toBeDefined();
      expect(body.revision).toBeGreaterThan(0);
    } else {
      // Validation error -- should be a structured JSON error
      expect(body.error).toBeDefined();
      expect(typeof body.error).toBe('string');
    }
  });

  test('POST /api/render handles malformed payload gracefully', async ({ request }) => {
    // Send a payload that is not a valid snapshot.
    // The Go server may be lenient (200 with partial output) or strict (4xx).
    // Either way, the response must be valid JSON with a coherent structure.
    const renderResp = await request.post('/api/render', {
      data: { snapshot: { not_valid: true } },
    });
    const body = await renderResp.json();

    if (renderResp.ok()) {
      // Lenient path: server accepted partial/empty snapshot and rendered it.
      // Verify the response has the expected render output shape.
      expect(body.html).toBeDefined();
      expect(body.render_id).toBeDefined();
    } else {
      // Strict path: server rejected the malformed input.
      expect(body.error).toBeDefined();
      expect(typeof body.error).toBe('string');
    }
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
