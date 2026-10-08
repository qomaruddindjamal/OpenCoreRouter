/**
 * pages/routing.js — Routing Table & Protocols Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5: Routing via FRRouting (frr-zebra, frr-ospfd, frr-bgpd, frr-ripd)
 * Static routes managed via UCI /etc/config/network or ip-full command.
 * OCR daemon calls FRR vtysh for protocol routes.
 */

'use strict';

import { GET, POST, DELETE } from '../core/api.js';
import { toast, toastError } from '../core/toast.js';
import { show as showModal, confirm as confirmDialog } from '../core/modal.js';
import { protoName, maskToCidr, badge } from '../core/utils.js';
import { registerPage }      from '../core/router.js';

export function initRouting() {
  registerPage('routing', loadRouting);
  document.getElementById('btn-add-route')?.addEventListener('click', showAddRouteModal);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadRouting() {
  await Promise.all([loadRouteTable(), loadProtocols()]);
}

/* ── Route table ─────────────────────────────────────────────── */
async function loadRouteTable() {
  const tbody = document.getElementById('route-tbody');
  if (!tbody) return;
  tbody.innerHTML = '<tr><td colspan="6" class="empty-row">Loading routes...</td></tr>';

  try {
    const data = await GET('/routing/table');
    renderRouteTable(tbody, data.routes ?? []);
  } catch (err) {
    toastError('Routing: ' + err.message);
  }
}

function renderRouteTable(tbody, routes) {
  if (!routes.length) {
    tbody.innerHTML = '<tr><td colspan="6" class="empty-row">No routes in table</td></tr>';
    return;
  }

  tbody.innerHTML = routes.map(r => {
    const dst    = r.Dst ? `${r.Dst.IP}/${maskToCidr(r.Dst.Mask)}` : '0.0.0.0/0';
    const gw     = r.Gw || '–';
    const iface  = r.LinkIndex ? `if${r.LinkIndex}` : '–';
    const proto  = protoName(r.Protocol);
    const metric = r.Priority ?? 0;

    // Only static routes can be deleted
    const canDel = r.Protocol === 1 || r.Protocol === 186;

    return `<tr>
      <td><strong>${dst}</strong></td>
      <td>${gw}</td>
      <td>${iface}</td>
      <td>${badge(proto, protoToBadgeClass(proto))}</td>
      <td>${metric}</td>
      <td>
        ${canDel ? `<button class="btn-sm btn-danger" data-del-route="${dst}" data-gw="${gw}">Delete</button>` : ''}
      </td>
    </tr>`;
  }).join('');

  tbody.querySelectorAll('[data-del-route]').forEach(btn => {
    btn.addEventListener('click', () => deleteRoute(btn.dataset.delRoute, btn.dataset.gw));
  });
}

function protoToBadgeClass(name) {
  const map = { STATIC: 'static', OSPF: 'ospf', BGP: 'bgp', KERNEL: 'down', CONNECTED: 'up' };
  return map[name] ?? 'static';
}

/* ── Delete static route ─────────────────────────────────────── */
async function deleteRoute(prefix, nexthop) {
  confirmDialog(
    `Delete route <strong>${prefix}</strong> via ${nexthop}?`,
    async () => {
      await DELETE(`/routing/static?prefix=${encodeURIComponent(prefix)}&nexthop=${encodeURIComponent(nexthop)}`);
      toast('Route deleted');
      loadRouteTable();
    },
    'Delete Route'
  );
}

/* ── Add static route modal ──────────────────────────────────── */
function showAddRouteModal() {
  showModal(
    'Add Static Route',
    `<div class="form-group">
       <label class="form-label">Destination Prefix</label>
       <input class="form-input" id="m-prefix" placeholder="10.0.0.0/8">
     </div>
     <div class="form-group">
       <label class="form-label">Next Hop / Gateway</label>
       <input class="form-input" id="m-nexthop" placeholder="192.168.1.1">
     </div>
     <div class="form-group">
       <label class="form-label">Interface (optional)</label>
       <input class="form-input" id="m-iface" placeholder="eth0">
     </div>
     <div class="form-group">
       <label class="form-label">Metric / Distance</label>
       <input class="form-input" id="m-metric" type="number" value="100" min="0" max="65535">
     </div>`,
    async () => {
      const body = {
        prefix:    document.getElementById('m-prefix').value.trim(),
        nexthop:   document.getElementById('m-nexthop').value.trim(),
        interface: document.getElementById('m-iface').value.trim() || undefined,
        metric:    parseInt(document.getElementById('m-metric').value) || 100,
      };
      if (!body.prefix) throw new Error('Destination prefix is required');
      if (!body.nexthop && !body.interface) throw new Error('Nexthop or interface required');
      await POST('/routing/static', body);
      toast('Static route added');
      loadRouteTable();
    },
    { submitLabel: 'Add Route' }
  );
}

/* ── Routing protocols ───────────────────────────────────────── */
async function loadProtocols() {
  const grid = document.getElementById('protocols-grid');
  if (!grid) return;

  try {
    const data = await GET('/routing/protocols');
    const protos = data.protocols ?? defaultProtocols();

    grid.innerHTML = protos.map(p => `
      <div class="protocol-card ${p.enabled ? 'active' : ''}">
        <div class="protocol-name" style="color:${p.enabled ? 'var(--accent-cyan)' : 'var(--text-muted)'}">
          ${p.name.toUpperCase()}
        </div>
        <div class="protocol-routes">${p.enabled ? (p.routes ?? 0) + ' routes' : 'Disabled'}</div>
        <div style="margin-top:8px">
          <button class="btn-sm ${p.enabled ? 'btn-warning' : 'btn-secondary'}"
            data-proto="${p.name}" data-enabled="${p.enabled}">
            ${p.enabled ? 'Disable' : 'Enable'}
          </button>
        </div>
      </div>
    `).join('');

    grid.querySelectorAll('[data-proto]').forEach(btn => {
      btn.addEventListener('click', () =>
        toggleProtocol(btn.dataset.proto, btn.dataset.enabled === 'true')
      );
    });
  } catch {
    // Silently degrade — protocols panel is optional
    grid.innerHTML = defaultProtocols().map(p => `
      <div class="protocol-card">
        <div class="protocol-name" style="color:var(--text-muted)">${p.name.toUpperCase()}</div>
        <div class="protocol-routes">Not installed</div>
      </div>
    `).join('');
  }
}

function defaultProtocols() {
  // FRRouting daemons available in OpenWrt 25.12.5 via apk
  return [
    { name: 'zebra',  enabled: false, routes: 0 },
    { name: 'ospf',   enabled: false, routes: 0 },
    { name: 'bgp',    enabled: false, routes: 0 },
    { name: 'rip',    enabled: false, routes: 0 },
    { name: 'static', enabled: true,  routes: 0 },
  ];
}

async function toggleProtocol(name, currentlyEnabled) {
  try {
    const action = currentlyEnabled ? 'disable' : 'enable';
    await POST(`/routing/protocols/${name}/${action}`);
    toast(`Protocol ${name.toUpperCase()} ${action}d`);
    loadProtocols();
  } catch (err) {
    toastError(err.message);
  }
}
