/**
 * components/chart.js — Traffic Chart (Canvas 2D)
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Renders a live TX/RX traffic sparkline on a <canvas> element.
 * Uses raw Canvas2D API — no external charting library required.
 */

'use strict';

import { fmtBps } from '../core/utils.js';
import { onStats } from '../core/websocket.js';

const HISTORY_LEN = 40;   // Data points to retain
const COLORS = {
  tx:       '#3B82F6',
  rx:       '#06B6D4',
  txFill:   'rgba(59,130,246,0.12)',
  rxFill:   'rgba(6,182,212,0.12)',
  grid:     'rgba(255,255,255,0.05)',
  label:    'rgba(255,255,255,0.3)',
};

/**
 * Create a traffic chart on the given canvas element.
 * @param {string|HTMLCanvasElement} canvasOrId
 * @returns {{ destroy: Function }}
 */
export function createTrafficChart(canvasOrId) {
  const canvas = typeof canvasOrId === 'string'
    ? document.getElementById(canvasOrId)
    : canvasOrId;

  if (!canvas) return { destroy: () => {} };

  const ctx = canvas.getContext('2d');

  const history = {
    tx: new Array(HISTORY_LEN).fill(0),
    rx: new Array(HISTORY_LEN).fill(0),
  };

  let _animId = null;
  let _dirty  = true;

  /** Push new data point */
  function push(txBps, rxBps) {
    history.tx.push(txBps ?? 0);
    history.rx.push(rxBps ?? 0);
    if (history.tx.length > HISTORY_LEN) history.tx.shift();
    if (history.rx.length > HISTORY_LEN) history.rx.shift();
    _dirty = true;
  }

  /** Draw a single line + fill */
  function drawLine(arr, max, color, fillColor) {
    const W = canvas.width;
    const H = canvas.height;
    const pad = 28; // bottom padding for labels
    const plotH = H - pad;

    ctx.beginPath();
    arr.forEach((v, i) => {
      const x = (i / (arr.length - 1)) * W;
      const y = plotH - (max > 0 ? (v / max) * (plotH - 10) : 0) + 5;
      i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
    });
    ctx.strokeStyle = color;
    ctx.lineWidth   = 2;
    ctx.lineJoin    = 'round';
    ctx.stroke();

    // Gradient fill
    ctx.lineTo(W, plotH); ctx.lineTo(0, plotH);
    ctx.closePath();
    const grad = ctx.createLinearGradient(0, 0, 0, plotH);
    grad.addColorStop(0, fillColor.replace(')', ', 0.4)').replace('rgba(', 'rgba('));
    grad.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = grad;
    ctx.fill();
  }

  /** Main render function */
  function render() {
    const W = canvas.offsetWidth;
    const H = 160;
    canvas.width  = W;
    canvas.height = H;

    ctx.clearRect(0, 0, W, H);

    const max = Math.max(...history.tx, ...history.rx, 1024);

    // Grid lines
    ctx.strokeStyle = COLORS.grid;
    ctx.lineWidth   = 1;
    for (let i = 1; i < 4; i++) {
      const y = ((H - 28) / 4) * i;
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(W, y); ctx.stroke();
    }

    drawLine(history.rx, max, COLORS.rx, COLORS.rxFill);
    drawLine(history.tx, max, COLORS.tx, COLORS.txFill);

    // Bottom labels
    ctx.fillStyle = COLORS.label;
    ctx.font      = '10px "JetBrains Mono", monospace';
    ctx.textAlign = 'left';
    ctx.fillText(`▲ TX: ${fmtBps(history.tx[history.tx.length - 1])}`, 8, H - 8);
    ctx.textAlign = 'right';
    ctx.fillText(`▼ RX: ${fmtBps(history.rx[history.rx.length - 1])}`, W - 8, H - 8);

    _dirty = false;
  }

  /** Animation loop */
  function loop() {
    if (_dirty) render();
    _animId = requestAnimationFrame(loop);
  }
  loop();

  // Subscribe to live stats
  const unsubscribe = onStats((data) => {
    // data.tx_bps / data.rx_bps from WebSocket
    push(data.tx_bps ?? 0, data.rx_bps ?? 0);
  });

  // Handle window resize
  const resizeObs = new ResizeObserver(() => { _dirty = true; });
  resizeObs.observe(canvas);

  return {
    push,
    destroy() {
      cancelAnimationFrame(_animId);
      unsubscribe();
      resizeObs.disconnect();
    },
  };
}

/**
 * Create a mini CPU/Memory donut-style gauge on a canvas
 * @param {string} canvasId
 * @param {object} opts
 * @param {string} opts.color
 */
export function createGaugeChart(canvasId, opts = {}) {
  const canvas = document.getElementById(canvasId);
  if (!canvas) return { update: () => {}, destroy: () => {} };

  const ctx   = canvas.getContext('2d');
  const color = opts.color ?? '#3B82F6';
  let _value  = 0;

  function render() {
    const size = Math.min(canvas.offsetWidth, canvas.offsetHeight);
    canvas.width = canvas.height = size;

    const cx   = size / 2;
    const cy   = size / 2;
    const r    = size * 0.38;
    const lw   = size * 0.12;
    const startAngle = -Math.PI / 2;
    const endAngle   = startAngle + (Math.PI * 2 * (_value / 100));

    ctx.clearRect(0, 0, size, size);

    // Track
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.strokeStyle = 'rgba(255,255,255,0.06)';
    ctx.lineWidth   = lw;
    ctx.stroke();

    // Value arc
    ctx.beginPath();
    ctx.arc(cx, cy, r, startAngle, endAngle);
    ctx.strokeStyle = color;
    ctx.lineWidth   = lw;
    ctx.lineCap     = 'round';
    ctx.stroke();

    // Center text
    ctx.fillStyle = '#e8edf5';
    ctx.font      = `bold ${Math.round(size * 0.2)}px Inter, sans-serif`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(`${Math.round(_value)}%`, cx, cy);
  }

  render();

  return {
    update(value) { _value = value; render(); },
    destroy() {},
  };
}
