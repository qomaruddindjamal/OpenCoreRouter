/**
 * pages/dhcp.js — DHCP Management Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5: dnsmasq-full handles DHCP + DNS (single process)
 * Config: UCI /etc/config/dhcp
 * Leases: /var/lib/misc/dnsmasq.leases
 */

'use strict';

import { GET, POST, DELETE } from '../core/api.js';
import { toast, toastError }   from '../core/toast.js';
import { show as showModal, confirm as confirmDialog } from '../core/modal.js';
import { fmtDate }             from '../core/utils.js';
import { registerPage }        from '../core/router.js';

export function initDHCP() {
  registerPage('dhcp', loadDHCP);
  document.getElementById('btn-add-pool')?.addEventListener('click', showAddPoolModal);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadDHCP() {
  await Promise.all([loadPools(), loadLeases(), loadStaticLeases()]);
}

/* ── DHCP Pools (dnsmasq-full UCI config) ────────────────────── */
async function loadPools() {
  const container = document.getElementById('dhcp-pools');
  if (!container) return;

  try {
    const data  = await GET('/dhcp/pools');
    const pools = data.pools ?? [];

    container.innerHTML = pools.length
      ? pools.map(p => `
          <div class="vpn-card">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px">
              <strong>${p.name}</strong>
              <span class="badge badge-${p.enabled ? 'up' : 'down'}">${p.enabled ? 'Active' : 'Disabled'}</span>
            </div>
            <div class="info-row"><span class="info-label">Interface</span><span class="info-val">${p.interface}</span></div>
            <div class="info-row"><span class="info-label">Range</span><span class="info-val">${p.start} – ${p.end}</span></div>
            <div class="info-row"><span class="info-label">Netmask</span><span class="info-val">${p.netmask ?? '255.255.255.0'}</span></div>
            <div class="info-row"><span class="info-label">Lease Time</span><span class="info-val">${p.leasetime ?? '12h'}</span></div>
            <div style="margin-top:10px">
              <button class="btn-sm btn-danger" data-del-pool="${p.name}">Delete Pool</button>
            </div>
          </div>
        `).join('')
      : '<div class="empty-state"><p>No DHCP pools configured</p></div>';

    container.querySelectorAll('[data-del-pool]').forEach(btn => {
      btn.addEventListener('click', () => deletePool(btn.dataset.delPool));
    });
  } catch (err) {
    toastError('DHCP pools: ' + err.message);
  }
}

/* ── Active Leases ───────────────────────────────────────────── */
async function loadLeases() {
  const tbody = document.getElementById('lease-tbody');
  if (!tbody) return;

  try {
    const data   = await GET('/dhcp/leases');
    const leases = data.leases ?? [];

    tbody.innerHTML = leases.length
      ? leases.map(l => `
          <tr>
            <td>${l.mac ?? '–'}</td>
            <td>${l.ip ?? '–'}</td>
            <td>${l.hostname || '–'}</td>
            <td>${fmtDate(l.expires)}</td>
            <td>
              <button class="btn-sm btn-secondary" data-reserve-lease="${l.mac}"
                data-ip="${l.ip}" data-hostname="${l.hostname}">Reserve</button>
            </td>
          </tr>
        `).join('')
      : '<tr><td colspan="5" class="empty-row">No active leases</td></tr>';

    tbody.querySelectorAll('[data-reserve-lease]').forEach(btn => {
      btn.addEventListener('click', () =>
        showReserveModal(btn.dataset.reserveLease, btn.dataset.ip, btn.dataset.hostname)
      );
    });
  } catch (err) {
    tbody.innerHTML = `<tr><td colspan="5" class="empty-row">Error: ${err.message}</td></tr>`;
  }
}

/* ── Static Leases (MAC → IP reservations) ───────────────────── */
async function loadStaticLeases() {
  const tbody = document.getElementById('static-lease-tbody');
  if (!tbody) return;

  try {
    const data = await GET('/dhcp/static');
    const list = data.leases ?? [];

    tbody.innerHTML = list.length
      ? list.map(l => `
          <tr>
            <td>${l.mac}</td>
            <td>${l.ip}</td>
            <td>${l.hostname || '–'}</td>
            <td><button class="btn-sm btn-danger" data-del-static="${l.mac}">Delete</button></td>
          </tr>
        `).join('')
      : '<tr><td colspan="4" class="empty-row">No static leases</td></tr>';

    tbody.querySelectorAll('[data-del-static]').forEach(btn => {
      btn.addEventListener('click', () => deleteStaticLease(btn.dataset.delStatic));
    });
  } catch {/* optional */ }
}

/* ── Modals ──────────────────────────────────────────────────── */
function showAddPoolModal() {
  showModal(
    'Add DHCP Pool',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:12px">
       dnsmasq-full manages DHCP via UCI <code>/etc/config/dhcp</code>
     </p>
     <div class="form-group">
       <label class="form-label">Pool Name</label>
       <input class="form-input" id="m-pool-name" placeholder="lan">
     </div>
     <div class="form-group">
       <label class="form-label">Interface</label>
       <input class="form-input" id="m-pool-iface" placeholder="br-lan  or  eth0">
     </div>
     <div class="form-group">
       <label class="form-label">Start IP (host part)</label>
       <input class="form-input" id="m-pool-start" type="number" value="100" min="1" max="254">
     </div>
     <div class="form-group">
       <label class="form-label">End IP (host part)</label>
       <input class="form-input" id="m-pool-end" type="number" value="250" min="1" max="254">
     </div>
     <div class="form-group">
       <label class="form-label">Netmask</label>
       <input class="form-input" id="m-pool-mask" value="255.255.255.0">
     </div>
     <div class="form-group">
       <label class="form-label">Lease Time</label>
       <input class="form-input" id="m-pool-lease" value="12h" placeholder="12h  /  1d  /  infinite">
     </div>`,
    async () => {
      const body = {
        name:       document.getElementById('m-pool-name').value.trim(),
        interface:  document.getElementById('m-pool-iface').value.trim(),
        start:      parseInt(document.getElementById('m-pool-start').value),
        end:        parseInt(document.getElementById('m-pool-end').value),
        netmask:    document.getElementById('m-pool-mask').value.trim(),
        leasetime:  document.getElementById('m-pool-lease').value.trim() || '12h',
      };
      if (!body.name || !body.interface) throw new Error('Name and interface required');
      await POST('/dhcp/pools', body);
      toast(`DHCP pool "${body.name}" created`);
      loadDHCP();
    },
    { submitLabel: 'Create Pool' }
  );
}

function showReserveModal(mac, ip, hostname) {
  showModal(
    'Reserve IP (Static Lease)',
    `<div class="form-group">
       <label class="form-label">MAC Address</label>
       <input class="form-input" id="m-res-mac" value="${mac}" readonly>
     </div>
     <div class="form-group">
       <label class="form-label">Reserved IP</label>
       <input class="form-input" id="m-res-ip" value="${ip}">
     </div>
     <div class="form-group">
       <label class="form-label">Hostname</label>
       <input class="form-input" id="m-res-host" value="${hostname || ''}">
     </div>`,
    async () => {
      await POST('/dhcp/static', {
        mac:      document.getElementById('m-res-mac').value,
        ip:       document.getElementById('m-res-ip').value.trim(),
        hostname: document.getElementById('m-res-host').value.trim() || undefined,
      });
      toast('Static lease created');
      loadDHCP();
    },
    { submitLabel: 'Reserve IP' }
  );
}

/* ── Delete actions ──────────────────────────────────────────── */
async function deletePool(name) {
  confirmDialog(`Delete DHCP pool "${name}"?`, async () => {
    await DELETE(`/dhcp/pools/${name}`);
    toast('Pool deleted');
    loadDHCP();
  }, 'Delete Pool');
}

async function deleteStaticLease(mac) {
  confirmDialog(`Delete static lease for ${mac}?`, async () => {
    await DELETE(`/dhcp/static/${mac}`);
    toast('Static lease deleted');
    loadStaticLeases();
  }, 'Delete Static Lease');
}
