/**
 * core/websocket.js — Real-time Stats via WebSocket
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Connects to ws(s)://<host>/ws/stats?token=<jwt>
 * Dispatches a custom event 'ocr:stats' with live data.
 */

'use strict';

import { getToken } from './auth.js';

const WS_PATH    = '/ws/stats';
const RECONNECT_MS = 5000;

let _ws          = null;
let _reconnectTimer = null;
let _running     = false;

/** Status indicator elements */
function setStatus(connected) {
  const dot  = document.getElementById('conn-status-dot');
  const text = document.getElementById('conn-status-text');
  if (dot)  dot.classList.toggle('active', connected);
  if (text) text.textContent = connected ? 'Connected' : 'Reconnecting...';
}

/** Dispatch a custom 'ocr:stats' event on document */
function dispatchStats(data) {
  document.dispatchEvent(new CustomEvent('ocr:stats', { detail: data }));
}

/** Build the WebSocket URL with JWT token */
function buildWsUrl() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  const token = getToken();
  return `${proto}://${location.host}${WS_PATH}?token=${token}`;
}

/** Open the WebSocket connection */
function connect() {
  if (!getToken()) return;

  _ws = new WebSocket(buildWsUrl());
  window._ocrWs = _ws;

  _ws.addEventListener('open', () => {
    setStatus(true);
    clearTimeout(_reconnectTimer);
  });

  _ws.addEventListener('message', (e) => {
    try {
      const data = JSON.parse(e.data);
      if (data.type === 'stats') dispatchStats(data);
    } catch {/* ignore malformed */ }
  });

  _ws.addEventListener('close', () => {
    setStatus(false);
    if (_running) {
      _reconnectTimer = setTimeout(connect, RECONNECT_MS);
    }
  });

  _ws.addEventListener('error', () => _ws.close());
}

/** Start the WebSocket connection (and auto-reconnect) */
export function startWebSocket() {
  _running = true;
  connect();
}

/** Stop and disconnect */
export function stopWebSocket() {
  _running = false;
  clearTimeout(_reconnectTimer);
  _ws?.close();
  _ws = null;
  window._ocrWs = null;
}

/** Subscribe to live stats events
 * @param {Function} handler  - receives stats data object
 * @returns {Function}        - unsubscribe function
 */
export function onStats(handler) {
  const listener = (e) => handler(e.detail);
  document.addEventListener('ocr:stats', listener);
  return () => document.removeEventListener('ocr:stats', listener);
}
