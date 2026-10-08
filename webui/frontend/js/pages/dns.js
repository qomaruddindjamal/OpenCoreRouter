/**
 * pages/dns.js — DNS Configuration Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5: dnsmasq-full handles DNS forwarding + local resolution
 * Config: UCI /etc/config/dhcp (dnsmasq section)
 * Supports: custom forwarders, local domain, hosts overrides, NXDOMAIN blocking
 */

'use strict';

import { GET, PUT, POST, DELETE } from '../core/api.js';
import { toast, toastError }        from '../core/toast.js';
import { show as showModal }        from '../core/modal.js';
import { registerPage }             from '../core/router.js';

export function initDNS() {
  registerPage('dns', loadDNS);
  document.getElementById('btn-save-dns')?.addEventListener('click', saveDNSConfig);
  document.getElementById('btn-add-host')?.addEventListener('click', showAddHostModal);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadDNS() {
  await Promise.all([loadDNSConfig(), loadHostOverrides()]);
}

/* ── DNS Config ──────────────────────────────────────────────── */
async function loadDNSConfig() {
  try {
    const cfg = await GET('/dns/config');

    const set = (id, val) => {
      const el = document.getElementById(id);
      if (el) el.value = val ?? '';
    };

    set('dns-listen',      cfg.listen_addr);
    set('dns-domain',      cfg.domain);
    set('dns-forwarders',  (cfg.forwarders ?? []).join('\n'));
    set('dns-search',      cfg.search_domain);

    // Checkboxes
    const setCheck = (id, val) => {
      const el = document.getElementById(id);
      if (el) el.checked = Boolean(val);
    };
    setCheck('dns-rebind-protection', cfg.rebind_protection);
    setCheck('dns-local-ttl',         cfg.local_ttl);
    setCheck('dns-nxdomain',          cfg.nxdomain_redirect);

  } catch (err) {
    toastError('DNS config: ' + err.message);
  }
}

async function saveDNSConfig() {
  try {
    const cfg = {
      listen_addr:         document.getElementById('dns-listen')?.value.trim(),
      domain:              document.getElementById('dns-domain')?.value.trim(),
      forwarders:          document.getElementById('dns-forwarders')?.value
                             .split('\n').map(s => s.trim()).filter(Boolean),
      search_domain:       document.getElementById('dns-search')?.value.trim(),
      rebind_protection:   document.getElementById('dns-rebind-protection')?.checked,
      nxdomain_redirect:   document.getElementById('dns-nxdomain')?.checked,
    };
    await PUT('/dns/config', cfg);
    toast('DNS configuration saved — dnsmasq reloaded');
  } catch (err) {
    toastError(err.message);
  }
}

/* ── Host Overrides (custom A records in dnsmasq) ────────────── */
async function loadHostOverrides() {
  const tbody = document.getElementById('dns-hosts-tbody');
  if (!tbody) return;

  try {
    const data  = await GET('/dns/hosts');
    const hosts = data.hosts ?? [];

    tbody.innerHTML = hosts.length
      ? hosts.map(h => `
          <tr>
            <td>${h.hostname}</td>
            <td>${h.ip}</td>
            <td>${h.description || '–'}</td>
            <td><button class="btn-sm btn-danger" data-del-host="${h.hostname}">Delete</button></td>
          </tr>
        `).join('')
      : '<tr><td colspan="4" class="empty-row">No host overrides</td></tr>';

    tbody.querySelectorAll('[data-del-host]').forEach(btn => {
      btn.addEventListener('click', () => deleteHost(btn.dataset.delHost));
    });
  } catch {
    tbody.innerHTML = '<tr><td colspan="4" class="empty-row">Host overrides unavailable</td></tr>';
  }
}

function showAddHostModal() {
  showModal(
    'Add DNS Host Override',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:12px">
       Adds a local A record to dnsmasq (UCI <code>/etc/config/dhcp</code> — host section).
     </p>
     <div class="form-group">
       <label class="form-label">Hostname (FQDN)</label>
       <input class="form-input" id="m-host-name" placeholder="mydevice.lan">
     </div>
     <div class="form-group">
       <label class="form-label">IP Address</label>
       <input class="form-input" id="m-host-ip" placeholder="192.168.1.50">
     </div>
     <div class="form-group">
       <label class="form-label">Description (optional)</label>
       <input class="form-input" id="m-host-desc" placeholder="">
     </div>`,
    async () => {
      const body = {
        hostname:    document.getElementById('m-host-name').value.trim(),
        ip:          document.getElementById('m-host-ip').value.trim(),
        description: document.getElementById('m-host-desc').value.trim() || undefined,
      };
      if (!body.hostname || !body.ip) throw new Error('Hostname and IP are required');
      await POST('/dns/hosts', body);
      toast(`Host ${body.hostname} → ${body.ip} added`);
      loadHostOverrides();
    },
    { submitLabel: 'Add Host' }
  );
}

async function deleteHost(hostname) {
  try {
    await DELETE(`/dns/hosts/${hostname}`);
    toast(`Host ${hostname} deleted`);
    loadHostOverrides();
  } catch (err) {
    toastError(err.message);
  }
}
