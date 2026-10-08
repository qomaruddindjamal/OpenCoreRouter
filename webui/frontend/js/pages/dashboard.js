/**
 * pages/dashboard.js — Dashboard Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

'use strict';

import { GET }             from '../core/api.js';
import { toastError }      from '../core/toast.js';
import { fmtBytes, fmtUptime, badge } from '../core/utils.js';
import { onStats }         from '../core/websocket.js';
import { createTrafficChart } from '../components/chart.js';
import { registerPage }    from '../core/router.js';

let _chart   = null;
let _unsubStats = null;

export function initDashboard() {
  registerPage('dashboard', loadDashboard);
}

async function loadDashboard() {
  // Init chart once
  if (!_chart) {
    _chart = createTrafficChart('traffic-chart');
  }

  // Subscribe to stats for CPU/mem bars
  _unsubStats?.();
  _unsubStats = onStats((data) => updateStatCards(data));

  try {
    const [sysInfo, ifaceData, routeData] = await Promise.all([
      GET('/system/info'),
      GET('/interfaces'),
      GET('/routing/table'),
    ]);

    renderSysInfo(sysInfo);
    renderStatsSummary(ifaceData, routeData);
    renderIfaceTable(ifaceData.interfaces ?? []);
  } catch (err) {
    if (err.status !== 401) toastError('Dashboard: ' + err.message);
  }
}

/* ── System info panel ───────────────────────────────────────── */
function renderSysInfo(info) {
  const set = (id, val) => {
    const el = document.getElementById(id);
    if (el) el.textContent = val ?? '--';
  };
  set('sys-hostname', info.hostname);
  set('sys-kernel',   info.kernel);
  set('sys-platform', info.platform);
  set('sys-uptime',   fmtUptime(info.uptime));
}

/* ── Stats cards: iface count + route count ─────────────────── */
function renderStatsSummary(ifaceData, routeData) {
  const ifaces = ifaceData.interfaces ?? [];
  const upCount = ifaces.filter(i => (i.Flags & 1)).length;

  const ifaceEl = document.getElementById('iface-count');
  const routeEl = document.getElementById('route-count');
  if (ifaceEl) ifaceEl.textContent = `${upCount}/${ifaces.length}`;
  if (routeEl) routeEl.textContent = routeData.count ?? 0;
}

/* ── Stat card bars (from WebSocket) ────────────────────────── */
function updateStatCards(data) {
  const cpuVal = document.getElementById('cpu-value');
  const cpuBar = document.getElementById('cpu-bar');
  const memVal = document.getElementById('mem-value');
  const memBar = document.getElementById('mem-bar');

  if (data.cpu_pct !== undefined) {
    const pct = data.cpu_pct.toFixed(1);
    if (cpuVal) cpuVal.textContent = pct + '%';
    if (cpuBar) cpuBar.style.width = pct + '%';
  }
  if (data.mem_pct !== undefined) {
    const pct = data.mem_pct.toFixed(1);
    if (memVal) memVal.textContent = pct + '%';
    if (memBar) memBar.style.width = pct + '%';
  }
}

/* ── Interfaces summary table ────────────────────────────────── */
function renderIfaceTable(ifaces) {
  const tbody = document.getElementById('iface-tbody');
  if (!tbody) return;

  if (!ifaces.length) {
    tbody.innerHTML = '<tr><td colspan="7" class="empty-row">No interfaces found</td></tr>';
    return;
  }

  tbody.innerHTML = ifaces.slice(0, 8).map(iface => {
    const isUp   = Boolean(iface.Flags & 1);
    const addrs  = (iface.Addresses ?? []).map(a => a.IP ?? a).join(', ') || '–';
    return `<tr>
      <td>${iface.Name}</td>
      <td>${badge(isUp ? 'UP' : 'DOWN', isUp ? 'up' : 'down')}</td>
      <td style="font-size:0.75rem">${addrs}</td>
      <td>${iface.MTU ?? '–'}</td>
      <td>${fmtBytes(iface.RxBytes)}</td>
      <td>${fmtBytes(iface.TxBytes)}</td>
      <td>
        <button class="btn-sm btn-secondary" data-iface="${iface.Name}" data-up="${!isUp}">
          ${isUp ? 'Down' : 'Up'}
        </button>
      </td>
    </tr>`;
  }).join('');

  // Bind toggle buttons
  tbody.querySelectorAll('button[data-iface]').forEach(btn => {
    btn.addEventListener('click', () => toggleIface(btn.dataset.iface, btn.dataset.up === 'true'));
  });

  // Sync button
  document.getElementById('btn-sync-ifaces')?.addEventListener('click', loadDashboard);
}

async function toggleIface(name, up) {
  try {
    await import('../core/api.js').then(({ PUT }) =>
      PUT(`/interfaces/${name}/${up ? 'up' : 'down'}`)
    );
    await loadDashboard();
  } catch (err) {
    toastError(err.message);
  }
}
