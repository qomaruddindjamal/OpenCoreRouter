/**
 * core/api.js — HTTP API Client
 * Semua request ke ocrd REST API melalui modul ini.
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

'use strict';

const API_BASE = window.location.origin + '/api/v1';

/** Ambil token dari store (diisi oleh auth.js) */
function getToken() {
  return window._ocrToken || localStorage.getItem('ocr_token');
}

/**
 * Core fetch wrapper
 * @param {string} method  - HTTP method
 * @param {string} path    - API path (e.g. '/interfaces')
 * @param {object} [body]  - Request body (JSON)
 * @returns {Promise<any>}
 */
async function request(method, path, body = null) {
  const opts = {
    method,
    headers: { 'Content-Type': 'application/json' },
  };

  const token = getToken();
  if (token) opts.headers['Authorization'] = 'Bearer ' + token;
  if (body)  opts.body = JSON.stringify(body);

  const res = await fetch(API_BASE + path, opts);
  const data = await res.json().catch(() => ({}));

  if (!res.ok) {
    const err = new Error(data.error || `HTTP ${res.status} ${res.statusText}`);
    err.status = res.status;
    throw err;
  }

  return data;
}

/** GET /api/v1/<path> */
export const GET    = (path)       => request('GET',    path);
/** POST /api/v1/<path> */
export const POST   = (path, body) => request('POST',   path, body);
/** PUT /api/v1/<path> */
export const PUT    = (path, body) => request('PUT',    path, body);
/** DELETE /api/v1/<path> */
export const DELETE = (path)       => request('DELETE', path);
/** PATCH /api/v1/<path> */
export const PATCH  = (path, body) => request('PATCH',  path, body);
