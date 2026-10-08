/**
 * pages/interfaces.js — Network Interfaces Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5: interfaces managed by netifd via UCI /etc/config/network
 * OCR daemon proxies netifd via ubus calls.
 */

'use strict';

import { GET, PUT, POST }  from '../core/api.js';
import { toast, toastError } from '../core/toast.js';
import { show as showModal }  from '../core/modal.js';
import { fmtBytes, badge }   from '../core/utils.js';
import { registerPage }      from '../core/router.js';

export function initInterfaces() {
  registerPage('interfaces', loadInterfaces);

  document.getElementById('btn-add-addr')?.addEventListener('click', showAddAddrModal);
  document.getElementById('btn-add-vlan')?.addEventListener('click', showAddVlanModal);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadInterfaces() {
  const tbody = document.getElementById('iface-full-tbody');
  if (!tbody) return;
  tbody.innerHTML = '<tr><td colspan="7" class="empty-row">Loading...</td></tr>';

  try {
    const data = await GET('/interfaces');
    renderTable(tbody, data.interfaces ?? []);
  } catch (err) {
    toastError('Interfaces: ' + err.message);
    tbody.innerHTML = `<tr><td colspan="7" class="empty-row">Error: ${err.message}</td></tr>`;
  }
}

/* ── Table renderer ──────────────────────────────────────────── */
function renderTable(tbody, ifaces) {
  if (!ifaces.length) {
    tbody.innerHTML = '<tr><td colspan="7" class="empty-row">No interfaces found</td></tr>';
    return;
  }

  tbody.innerHTML = ifaces.map(iface => {
    const isUp  = Boolean(iface.Flags & 1);
    const mac   = iface.HardwareAddr
      ? (Array.isArray(iface.HardwareAddr)
        ? iface.HardwareAddr.map(b => b.toString(16).padStart(2, '0')).join(':')
        : iface.HardwareAddr)
      : '--';
    const addrs = (iface.Addresses ?? [])
      .map(a => `<code class="addr-badge">${a.IP ?? a}</code>`)
      .join(' ') || '–';

    return `<tr>
      <td><strong>${iface.Name}</strong></td>
      <td>${iface.Index}</td>
      <td>${badge(isUp ? 'UP' : 'DOWN', isUp ? 'up' : 'down')}</td>
      <td style="font-family:monospace;font-size:0.75rem">${mac}</td>
      <td>${iface.MTU ?? '–'}</td>
      <td style="max-width:220px">${addrs}</td>
      <td>
        <div style="display:flex;gap:6px;flex-wrap:wrap">
          <button class="btn-sm btn-secondary" data-action="toggle"
            data-iface="${iface.Name}" data-up="${!isUp}">
            ${isUp ? 'Set Down' : 'Set Up'}
          </button>
          <button class="btn-sm btn-primary" data-action="add-addr"
            data-iface="${iface.Name}">+Addr</button>
          <button class="btn-sm btn-warning" data-action="set-mtu"
            data-iface="${iface.Name}" data-mtu="${iface.MTU ?? 1500}">MTU</button>
        </div>
      </td>
    </tr>`;
  }).join('');

  // Bind action buttons
  tbody.querySelectorAll('button[data-action]').forEach(btn => {
    btn.addEventListener('click', () => handleAction(btn));
  });
}

/* ── Action dispatcher ───────────────────────────────────────── */
async function handleAction(btn) {
  const { action, iface, up, mtu } = btn.dataset;
  switch (action) {
    case 'toggle':   return toggleIface(iface, up === 'true');
    case 'add-addr': return showAddAddrModal(iface);
    case 'set-mtu':  return showSetMtuModal(iface, mtu);
  }
}

/* ── Toggle UP/DOWN ──────────────────────────────────────────── */
async function toggleIface(name, up) {
  try {
    await PUT(`/interfaces/${name}/${up ? 'up' : 'down'}`);
    toast(`${name} is now ${up ? 'UP' : 'DOWN'}`);
    await loadInterfaces();
  } catch (err) {
    toastError(err.message);
  }
}

/* ── Add address modal ───────────────────────────────────────── */
function showAddAddrModal(iface = '') {
  showModal(
    'Add IP Address',
    `<div class="form-group">
       <label class="form-label">Interface</label>
       <input class="form-input" id="m-iface-name" value="${iface}" ${iface ? 'readonly' : ''}>
     </div>
     <div class="form-group">
       <label class="form-label">CIDR (e.g. 192.168.1.1/24)</label>
       <input class="form-input" id="m-cidr" placeholder="192.168.1.1/24">
     </div>`,
    async () => {
      const ifaceName = document.getElementById('m-iface-name').value.trim();
      const cidr = document.getElementById('m-cidr').value.trim();
      if (!ifaceName || !cidr) throw new Error('Interface and CIDR are required');
      await POST(`/interfaces/${ifaceName}/addresses`, { cidr });
      toast(`Address ${cidr} added to ${ifaceName}`);
      loadInterfaces();
    },
    { submitLabel: 'Add Address' }
  );
}

/* ── Set MTU modal ───────────────────────────────────────────── */
function showSetMtuModal(iface, currentMtu) {
  showModal(
    `Set MTU — ${iface}`,
    `<div class="form-group">
       <label class="form-label">MTU (bytes)</label>
       <input class="form-input" id="m-mtu" type="number" min="68" max="65535" value="${currentMtu}">
       <small style="color:var(--text-muted)">Standard: 1500 (Ethernet), 9000 (Jumbo)</small>
     </div>`,
    async () => {
      const mtu = parseInt(document.getElementById('m-mtu').value);
      if (!mtu || mtu < 68) throw new Error('MTU must be ≥ 68');
      await PUT(`/interfaces/${iface}/mtu`, { mtu });
      toast(`MTU set to ${mtu} on ${iface}`);
      loadInterfaces();
    },
    { submitLabel: 'Set MTU' }
  );
}

/* ── Add VLAN modal ──────────────────────────────────────────── */
function showAddVlanModal() {
  showModal(
    'Add VLAN Interface',
    `<div class="form-group">
       <label class="form-label">Parent Interface</label>
       <input class="form-input" id="m-vlan-parent" placeholder="eth0">
     </div>
     <div class="form-group">
       <label class="form-label">VLAN ID (1–4094)</label>
       <input class="form-input" id="m-vlan-id" type="number" min="1" max="4094" placeholder="100">
     </div>`,
    async () => {
      const parent = document.getElementById('m-vlan-parent').value.trim();
      const vid    = parseInt(document.getElementById('m-vlan-id').value);
      if (!parent || !vid) throw new Error('Parent interface and VLAN ID required');
      await POST('/interfaces/vlan', { parent, vlan_id: vid });
      toast(`VLAN ${vid} created on ${parent}`);
      loadInterfaces();
    },
    { submitLabel: 'Create VLAN' }
  );
}
