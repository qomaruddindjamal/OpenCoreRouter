/**
 * pages/qos.js — QoS (Quality of Service) Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5: sqm-scripts with CAKE algorithm (kmod-sched-cake)
 * Config: UCI /etc/config/sqm
 * Also supports HTB (Hierarchical Token Bucket) via tc-full
 */

'use strict';

import { GET, POST, PUT, DELETE } from '../core/api.js';
import { toast, toastError }        from '../core/toast.js';
import { show as showModal, confirm as confirmDialog } from '../core/modal.js';
import { fmtBps }                   from '../core/utils.js';
import { registerPage }             from '../core/router.js';

export function initQoS() {
  registerPage('qos', loadQoS);
  document.getElementById('btn-add-qos')?.addEventListener('click',   showAddQoSModal);
  document.getElementById('btn-apply-qos')?.addEventListener('click', applyQoS);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadQoS() {
  await Promise.all([loadPolicies(), loadQoSStatus()]);
}

/* ── QoS policies ────────────────────────────────────────────── */
async function loadPolicies() {
  const tbody = document.getElementById('qos-tbody');
  if (!tbody) return;
  tbody.innerHTML = '<tr><td colspan="6" class="empty-row">Loading...</td></tr>';

  try {
    const data     = await GET('/qos/policies');
    const policies = data.policies ?? [];

    if (!policies.length) {
      tbody.innerHTML = `
        <tr><td colspan="6" class="empty-row">
          No QoS policies.<br>
          <small style="color:var(--text-muted)">
            Uses sqm-scripts + CAKE algorithm (OpenWrt 25.12.5)
          </small>
        </td></tr>`;
      return;
    }

    tbody.innerHTML = policies.map(p => `
      <tr>
        <td><strong>${p.name}</strong></td>
        <td>${p.interface}</td>
        <td>${fmtBps(p.download * 1000)}</td>
        <td>${fmtBps(p.upload * 1000)}</td>
        <td>
          <span class="badge ${p.algorithm === 'cake' ? 'badge-up' : 'badge-static'}">
            ${(p.algorithm ?? 'cake').toUpperCase()}
          </span>
        </td>
        <td>
          <div style="display:flex;gap:6px">
            <button class="btn-sm btn-secondary" data-edit-qos="${p.name}" data-qos='${JSON.stringify(p)}'>Edit</button>
            <button class="btn-sm btn-danger" data-del-qos="${p.name}">Delete</button>
          </div>
        </td>
      </tr>
    `).join('');

    tbody.querySelectorAll('[data-del-qos]').forEach(btn => {
      btn.addEventListener('click', () => deletePolicy(btn.dataset.delQos));
    });
    tbody.querySelectorAll('[data-edit-qos]').forEach(btn => {
      btn.addEventListener('click', () => showAddQoSModal(JSON.parse(btn.dataset.qos)));
    });

  } catch (err) {
    toastError('QoS: ' + err.message);
    tbody.innerHTML = `<tr><td colspan="6" class="empty-row">Error: ${err.message}</td></tr>`;
  }
}

/* ── SQM / CAKE status ───────────────────────────────────────── */
async function loadQoSStatus() {
  const el = document.getElementById('qos-status');
  if (!el) return;
  try {
    const data = await GET('/qos/status');
    el.textContent = data.enabled ? 'SQM Active' : 'SQM Inactive';
    el.className   = `badge ${data.enabled ? 'badge-up' : 'badge-down'}`;
  } catch { /* optional */ }
}

/* ── Add / Edit modal ────────────────────────────────────────── */
function showAddQoSModal(existing = null) {
  const isEdit = Boolean(existing);
  showModal(
    isEdit ? `Edit QoS — ${existing.name}` : 'Add QoS Policy',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:12px">
       Uses <strong>CAKE</strong> algorithm via sqm-scripts (OpenWrt 25.12.5).<br>
       Requires: <code>apk add sqm-scripts kmod-sched-cake tc-full</code>
     </p>
     <div class="form-group">
       <label class="form-label">Policy Name</label>
       <input class="form-input" id="m-qos-name" value="${existing?.name ?? ''}"
         placeholder="lan-qos" ${isEdit ? 'readonly' : ''}>
     </div>
     <div class="form-group">
       <label class="form-label">Interface</label>
       <input class="form-input" id="m-qos-iface" value="${existing?.interface ?? ''}"
         placeholder="eth0  /  br-lan  /  pppoe-wan">
     </div>
     <div class="form-group">
       <label class="form-label">Download Limit (kbps, 0 = unlimited)</label>
       <input class="form-input" id="m-qos-dl" type="number" value="${existing?.download ?? 0}" min="0">
     </div>
     <div class="form-group">
       <label class="form-label">Upload Limit (kbps, 0 = unlimited)</label>
       <input class="form-input" id="m-qos-ul" type="number" value="${existing?.upload ?? 0}" min="0">
     </div>
     <div class="form-group">
       <label class="form-label">Algorithm</label>
       <select class="form-select" id="m-qos-algo">
         <option value="cake" ${existing?.algorithm === 'cake' ? 'selected' : ''}>CAKE (Recommended)</option>
         <option value="fq_codel" ${existing?.algorithm === 'fq_codel' ? 'selected' : ''}>FQ-CoDel</option>
         <option value="htb" ${existing?.algorithm === 'htb' ? 'selected' : ''}>HTB (Hierarchical)</option>
       </select>
     </div>`,
    async () => {
      const body = {
        name:      document.getElementById('m-qos-name').value.trim(),
        interface: document.getElementById('m-qos-iface').value.trim(),
        download:  parseInt(document.getElementById('m-qos-dl').value) || 0,
        upload:    parseInt(document.getElementById('m-qos-ul').value) || 0,
        algorithm: document.getElementById('m-qos-algo').value,
      };
      if (!body.name || !body.interface) throw new Error('Name and interface required');

      if (isEdit) {
        await PUT(`/qos/policies/${body.name}`, body);
        toast(`Policy ${body.name} updated`);
      } else {
        await POST('/qos/policies', body);
        toast(`Policy ${body.name} created`);
      }
      loadQoS();
    },
    { submitLabel: isEdit ? 'Update Policy' : 'Create Policy' }
  );
}

async function deletePolicy(name) {
  confirmDialog(`Delete QoS policy "${name}"?`, async () => {
    await DELETE(`/qos/policies/${name}`);
    toast('Policy deleted');
    loadQoS();
  }, 'Delete QoS');
}

async function applyQoS() {
  try {
    await POST('/qos/apply');
    toast('QoS applied — sqm-scripts reloaded');
  } catch (err) {
    toastError(err.message);
  }
}
