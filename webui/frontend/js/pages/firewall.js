/**
 * pages/firewall.js — Firewall Rules Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5: firewall4 → nftables backend
 * UCI config: /etc/config/firewall
 * OCR daemon applies rules via fw4 / nft commands.
 */

'use strict';

import { GET, POST, DELETE } from '../core/api.js';
import { toast, toastError }   from '../core/toast.js';
import { show as showModal, confirm as confirmDialog } from '../core/modal.js';
import { badge }               from '../core/utils.js';
import { registerPage }        from '../core/router.js';

export function initFirewall() {
  registerPage('firewall', loadFirewall);

  document.getElementById('btn-add-rule')?.addEventListener('click', showAddRuleModal);
  document.getElementById('btn-apply-fw')?.addEventListener('click', applyRules);
  document.getElementById('btn-flush-fw')?.addEventListener('click', flushRules);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadFirewall() {
  await Promise.all([loadFilterRules(), loadNatRules()]);
}

/* ── Filter rules ────────────────────────────────────────────── */
async function loadFilterRules() {
  const tbody = document.getElementById('fw-tbody');
  if (!tbody) return;
  tbody.innerHTML = '<tr><td colspan="8" class="empty-row">Loading...</td></tr>';

  try {
    const data  = await GET('/firewall/rules');
    const rules = data.rules ?? [];

    if (!rules.length) {
      tbody.innerHTML = '<tr><td colspan="8" class="empty-row">No firewall rules configured</td></tr>';
      return;
    }

    tbody.innerHTML = rules.map((r, idx) => `
      <tr>
        <td>${r.id ?? idx + 1}</td>
        <td>${r.direction ?? 'input'}</td>
        <td>${r.source || 'any'}</td>
        <td>${r.destination || 'any'}</td>
        <td>${r.protocol || 'all'}</td>
        <td>${badge(r.action?.toUpperCase() ?? 'ACCEPT', r.action ?? 'accept')}</td>
        <td>
          <label class="toggle-switch">
            <input type="checkbox" ${r.enabled ? 'checked' : ''} data-rule-id="${r.id}" data-toggle-rule>
          </label>
        </td>
        <td>
          <button class="btn-sm btn-danger" data-del-rule="${r.id}">Delete</button>
        </td>
      </tr>
    `).join('');

    tbody.querySelectorAll('[data-del-rule]').forEach(btn => {
      btn.addEventListener('click', () => deleteRule(btn.dataset.delRule));
    });

    tbody.querySelectorAll('[data-toggle-rule]').forEach(cb => {
      cb.addEventListener('change', () => toggleRule(cb.dataset.ruleId, cb.checked));
    });

  } catch (err) {
    toastError('Firewall: ' + err.message);
    tbody.innerHTML = `<tr><td colspan="8" class="empty-row">Error: ${err.message}</td></tr>`;
  }
}

/* ── NAT rules ───────────────────────────────────────────────── */
async function loadNatRules() {
  const tbody = document.getElementById('nat-tbody');
  if (!tbody) return;

  try {
    const data  = await GET('/firewall/nat');
    const rules = data.rules ?? [];

    tbody.innerHTML = rules.length
      ? rules.map((r, idx) => `
          <tr>
            <td>${idx + 1}</td>
            <td>${badge(r.type ?? 'SNAT', 'static')}</td>
            <td>${r.source || 'any'}</td>
            <td>${r.destination || 'any'}</td>
            <td>${r.to_ip || r.out_iface || '–'}</td>
            <td><button class="btn-sm btn-danger" data-del-nat="${r.id}">Delete</button></td>
          </tr>
        `).join('')
      : '<tr><td colspan="6" class="empty-row">No NAT rules configured</td></tr>';

    tbody.querySelectorAll('[data-del-nat]').forEach(btn => {
      btn.addEventListener('click', () => deleteNat(btn.dataset.delNat));
    });
  } catch {
    tbody.innerHTML = '<tr><td colspan="6" class="empty-row">NAT rules unavailable</td></tr>';
  }
}

/* ── Add rule modal ──────────────────────────────────────────── */
function showAddRuleModal() {
  showModal(
    'Add Firewall Rule',
    `<div class="form-group">
       <label class="form-label">Direction / Hook</label>
       <select class="form-select" id="m-dir">
         <option value="input">INPUT (inbound to router)</option>
         <option value="output">OUTPUT (from router)</option>
         <option value="forward">FORWARD (through router)</option>
       </select>
     </div>
     <div class="form-group">
       <label class="form-label">Source Zone / IP (CIDR)</label>
       <input class="form-input" id="m-src" placeholder="wan  or  192.168.0.0/24">
     </div>
     <div class="form-group">
       <label class="form-label">Destination Zone / IP (CIDR)</label>
       <input class="form-input" id="m-dst" placeholder="lan  or  10.0.0.0/8">
     </div>
     <div class="form-group">
       <label class="form-label">Protocol</label>
       <select class="form-select" id="m-proto">
         <option value="all">all</option>
         <option value="tcp">TCP</option>
         <option value="udp">UDP</option>
         <option value="tcpudp">TCP+UDP</option>
         <option value="icmp">ICMP</option>
       </select>
     </div>
     <div class="form-group">
       <label class="form-label">Destination Port (e.g. 80 or 8000-9000)</label>
       <input class="form-input" id="m-dport" placeholder="optional">
     </div>
     <div class="form-group">
       <label class="form-label">Action</label>
       <select class="form-select" id="m-action">
         <option value="accept">ACCEPT</option>
         <option value="drop">DROP</option>
         <option value="reject">REJECT</option>
       </select>
     </div>`,
    async () => {
      const body = {
        direction:   document.getElementById('m-dir').value,
        source:      document.getElementById('m-src').value.trim() || 'any',
        destination: document.getElementById('m-dst').value.trim() || 'any',
        protocol:    document.getElementById('m-proto').value,
        dst_port:    document.getElementById('m-dport').value.trim() || undefined,
        action:      document.getElementById('m-action').value,
        enabled:     true,
      };
      await POST('/firewall/rules', body);
      toast('Firewall rule added');
      loadFilterRules();
    },
    { submitLabel: 'Add Rule' }
  );
}

/* ── Actions ─────────────────────────────────────────────────── */
async function deleteRule(id) {
  confirmDialog(`Delete firewall rule #${id}?`, async () => {
    await DELETE(`/firewall/rules/${id}`);
    toast('Rule deleted');
    loadFilterRules();
  }, 'Delete Rule');
}

async function toggleRule(id, enabled) {
  try {
    await POST(`/firewall/rules/${id}/toggle`, { enabled });
    toast(`Rule ${enabled ? 'enabled' : 'disabled'}`);
  } catch (err) {
    toastError(err.message);
  }
}

async function deleteNat(id) {
  confirmDialog(`Delete NAT rule #${id}?`, async () => {
    await DELETE(`/firewall/nat/${id}`);
    toast('NAT rule deleted');
    loadNatRules();
  }, 'Delete NAT');
}

async function applyRules() {
  try {
    await POST('/firewall/apply');
    toast('Firewall rules applied via firewall4');
  } catch (err) {
    toastError(err.message);
  }
}

async function flushRules() {
  confirmDialog(
    'Flush ALL firewall rules? This will remove all nftables rules.',
    async () => {
      await POST('/firewall/flush');
      toast('Firewall flushed', 'warning');
      loadFirewall();
    },
    'Flush Firewall'
  );
}
