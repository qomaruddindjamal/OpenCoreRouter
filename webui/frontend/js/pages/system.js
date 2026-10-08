/**
 * pages/system.js — System Information & Control Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5:
 *   Info   → /proc/version, /proc/uptime, uci system config
 *   Reboot → procd: /sbin/reboot
 *   Shutdown → /sbin/poweroff
 *   Packages → apk (Alpine Package Keeper, migrated from opkg in 25.x)
 *   Logs   → logread (logd)
 */

'use strict';

import { GET, POST }         from '../core/api.js';
import { toast, toastError } from '../core/toast.js';
import { show as showModal, confirm as confirmDialog } from '../core/modal.js';
import { fmtUptime, fmtDate } from '../core/utils.js';
import { registerPage }       from '../core/router.js';

export function initSystem() {
  registerPage('system', loadSystem);

  document.getElementById('btn-reboot')?.addEventListener('click',   confirmReboot);
  document.getElementById('btn-shutdown')?.addEventListener('click', confirmShutdown);
  document.getElementById('btn-save-hostname')?.addEventListener('click', saveHostname);
  document.getElementById('btn-pkg-install')?.addEventListener('click', showPkgModal);
  document.getElementById('btn-show-logs')?.addEventListener('click',   loadLogs);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadSystem() {
  await Promise.all([loadSysInfo(), loadPackages(), loadLogs()]);
}

/* ── System info ─────────────────────────────────────────────── */
async function loadSysInfo() {
  const container = document.getElementById('sys-full-info');
  if (!container) return;

  try {
    const info = await GET('/system/info');

    const rows = [
      { label: 'Hostname',      value: info.hostname ?? '--' },
      { label: 'Platform',      value: `${info.platform ?? '--'} (x86_64)` },
      { label: 'Kernel',        value: `Linux ${info.kernel ?? '--'}` },
      { label: 'OpenWrt',       value: info.openwrt_version ?? '25.12.5' },
      { label: 'Uptime',        value: fmtUptime(info.uptime) },
      { label: 'Load Average',  value: info.loadavg ?? '--' },
      { label: 'Total Memory',  value: fmtMem(info.mem_total) },
      { label: 'Free Memory',   value: fmtMem(info.mem_free) },
      { label: 'Package Mgr',   value: 'apk (Alpine Package Keeper)' },
      { label: 'Init System',   value: 'procd' },
      { label: 'Firewall',      value: 'firewall4 → nftables' },
      { label: 'DHCP/DNS',      value: 'dnsmasq-full' },
      { label: 'Routing',       value: 'FRRouting (frr-zebra)' },
    ];

    container.innerHTML = rows.map(r => `
      <div class="info-row">
        <span class="info-label">${r.label}</span>
        <span class="info-val">${r.value}</span>
      </div>
    `).join('');

    // Pre-fill hostname input
    const hostnameInput = document.getElementById('sys-hostname-input');
    if (hostnameInput) hostnameInput.value = info.hostname ?? '';

  } catch (err) {
    toastError('System info: ' + err.message);
  }
}

function fmtMem(kb) {
  if (!kb) return '--';
  return `${(kb / 1024).toFixed(0)} MB`;
}

/* ── Hostname change ─────────────────────────────────────────── */
async function saveHostname() {
  const input = document.getElementById('sys-hostname-input');
  if (!input) return;
  try {
    await POST('/system/hostname', { hostname: input.value.trim() });
    toast('Hostname updated — changes take effect after reboot');
  } catch (err) {
    toastError(err.message);
  }
}

/* ── Packages (apk) ──────────────────────────────────────────── */
async function loadPackages() {
  const tbody = document.getElementById('pkg-tbody');
  if (!tbody) return;

  try {
    const data = await GET('/system/packages');
    const pkgs = data.packages ?? [];

    tbody.innerHTML = pkgs.length
      ? pkgs.map(p => `
          <tr>
            <td>${p.name}</td>
            <td>${p.version ?? '--'}</td>
            <td>${p.size ?? '--'}</td>
            <td><span class="badge badge-up">Installed</span></td>
          </tr>
        `).join('')
      : '<tr><td colspan="4" class="empty-row">No packages found</td></tr>';
  } catch {
    tbody.innerHTML = '<tr><td colspan="4" class="empty-row">Package list unavailable</td></tr>';
  }
}

function showPkgModal() {
  showModal(
    'Install Package (apk)',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:12px">
       OpenWrt 25.12.5 uses <code>apk</code> (Alpine Package Keeper).<br>
       Run: <code>apk update && apk add &lt;package&gt;</code>
     </p>
     <div class="form-group">
       <label class="form-label">Package Name</label>
       <input class="form-input" id="m-pkg-name" placeholder="e.g. php83, frr-ospfd, sqm-scripts">
     </div>`,
    async () => {
      const pkg = document.getElementById('m-pkg-name').value.trim();
      if (!pkg) throw new Error('Package name required');
      await POST('/system/packages/install', { package: pkg });
      toast(`Installing ${pkg}…`);
      setTimeout(loadPackages, 3000);
    },
    { submitLabel: 'Install via apk' }
  );
}

/* ── System log (logread) ────────────────────────────────────── */
async function loadLogs() {
  const el = document.getElementById('sys-log');
  if (!el) return;
  el.textContent = 'Loading logs…';

  try {
    const data = await GET('/system/log?lines=100');
    el.textContent = data.log ?? 'No logs available';
    el.scrollTop   = el.scrollHeight;
  } catch {
    el.textContent = 'Log unavailable (logd)';
  }
}

/* ── Reboot / Shutdown ───────────────────────────────────────── */
function confirmReboot() {
  confirmDialog(
    'Reboot the router now? All connections will be interrupted.',
    async () => {
      await POST('/system/reboot');
      toast('Rebooting via procd…', 'warning');
    },
    'Confirm Reboot'
  );
}

function confirmShutdown() {
  confirmDialog(
    'Shutdown the router? You will need physical access to restart it.',
    async () => {
      await POST('/system/shutdown');
      toast('Shutting down…', 'warning');
    },
    'Confirm Shutdown'
  );
}
