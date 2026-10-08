/**
 * pages/vpn.js — VPN Management Page
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * OpenWrt 25.12.5:
 *   WireGuard  → kmod-wireguard + wireguard-tools (native kernel module)
 *   OpenVPN    → openvpn-openssl (apk install openvpn-openssl)
 *   IPSec      → strongswan (apk install strongswan)
 * WireGuard config via UCI /etc/config/network (proto wireguard)
 */

'use strict';

import { GET, POST, DELETE } from '../core/api.js';
import { toast, toastError }   from '../core/toast.js';
import { show as showModal, confirm as confirmDialog } from '../core/modal.js';
import { badge }               from '../core/utils.js';
import { registerPage }        from '../core/router.js';

export function initVPN() {
  registerPage('vpn', loadVPN);

  document.getElementById('btn-add-wg')?.addEventListener('click',   showWireGuardModal);
  document.getElementById('btn-add-ovpn')?.addEventListener('click', showOpenVPNModal);
  document.getElementById('btn-add-ipsec')?.addEventListener('click', showIPSecModal);
}

/* ── Page load ───────────────────────────────────────────────── */
async function loadVPN() {
  const grid = document.getElementById('vpn-grid');
  if (!grid) return;

  try {
    const data    = await GET('/vpn/tunnels');
    const tunnels = data.tunnels ?? [];

    if (!tunnels.length) {
      grid.innerHTML = emptyState();
      return;
    }

    grid.innerHTML = tunnels.map(t => tunnelCard(t)).join('');

    grid.querySelectorAll('[data-del-tunnel]').forEach(btn => {
      btn.addEventListener('click', () => deleteTunnel(btn.dataset.delTunnel, btn.dataset.type));
    });

    grid.querySelectorAll('[data-peer-tunnel]').forEach(btn => {
      btn.addEventListener('click', () => showAddPeerModal(btn.dataset.peerTunnel));
    });

  } catch (err) {
    toastError('VPN: ' + err.message);
    grid.innerHTML = `<div class="empty-state"><p>Error: ${err.message}</p></div>`;
  }
}

/* ── Tunnel card HTML ────────────────────────────────────────── */
function tunnelCard(t) {
  const typeColor = { wireguard: 'var(--accent-cyan)', openvpn: 'var(--accent-green)', ipsec: 'var(--accent-purple)' };
  const color = typeColor[t.type] ?? 'var(--accent-blue)';

  return `
    <div class="vpn-card">
      <div class="vpn-card-header">
        <div>
          <span class="vpn-type-badge" style="color:${color}">${(t.type ?? 'vpn').toUpperCase()}</span>
          <strong style="margin-left:8px">${t.name}</strong>
        </div>
        ${badge(t.connected ? 'UP' : 'DOWN', t.connected ? 'up' : 'down')}
      </div>
      <div class="info-row"><span class="info-label">Address</span><span class="info-val">${t.address ?? '–'}</span></div>
      <div class="info-row"><span class="info-label">Listen Port</span><span class="info-val">${t.port ?? '–'}</span></div>
      <div class="info-row"><span class="info-label">Peers</span><span class="info-val">${t.peer_count ?? 0}</span></div>
      ${t.public_key ? `<div class="info-row"><span class="info-label">Public Key</span><span class="info-val" style="font-size:0.65rem">${t.public_key.slice(0, 20)}…</span></div>` : ''}
      <div style="display:flex;gap:8px;margin-top:12px">
        ${t.type === 'wireguard' ? `<button class="btn-sm btn-secondary" data-peer-tunnel="${t.name}">+ Peer</button>` : ''}
        <button class="btn-sm btn-danger" data-del-tunnel="${t.name}" data-type="${t.type}">Delete</button>
      </div>
    </div>`;
}

function emptyState() {
  return `<div class="empty-state">
    <svg viewBox="0 0 64 64" fill="none">
      <circle cx="32" cy="32" r="30" stroke="currentColor" stroke-width="2" opacity="0.3"/>
      <path d="M20 32a12 12 0 0 1 24 0" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
    </svg>
    <p>No VPN tunnels configured</p>
    <p class="empty-sub">OpenWrt 25.12.5 supports WireGuard (native), OpenVPN, and IPSec</p>
  </div>`;
}

/* ── WireGuard modal ─────────────────────────────────────────── */
function showWireGuardModal() {
  showModal(
    'Create WireGuard Interface',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:16px">
       Uses <code>kmod-wireguard</code> (native kernel module in OpenWrt 25.12.5).
       Config stored in UCI <code>/etc/config/network</code>.
     </p>
     <div class="form-group">
       <label class="form-label">Interface Name</label>
       <input class="form-input" id="m-wg-name" value="wg0">
     </div>
     <div class="form-group">
       <label class="form-label">Address (CIDR)</label>
       <input class="form-input" id="m-wg-addr" placeholder="10.0.0.1/24">
     </div>
     <div class="form-group">
       <label class="form-label">Listen Port</label>
       <input class="form-input" id="m-wg-port" type="number" value="51820" min="1" max="65535">
     </div>
     <div class="form-group">
       <label class="form-label">Private Key (leave blank to auto-generate)</label>
       <input class="form-input" id="m-wg-key" placeholder="auto-generated via wg genkey">
     </div>`,
    async () => {
      const body = {
        name:         document.getElementById('m-wg-name').value.trim(),
        address:      document.getElementById('m-wg-addr').value.trim(),
        listen_port:  parseInt(document.getElementById('m-wg-port').value) || 51820,
        private_key:  document.getElementById('m-wg-key').value.trim() || undefined,
      };
      if (!body.name)    throw new Error('Interface name is required');
      if (!body.address) throw new Error('Address CIDR is required');
      await POST('/vpn/wireguard', body);
      toast(`WireGuard ${body.name} created`);
      loadVPN();
    },
    { submitLabel: 'Create WireGuard' }
  );
}

/* ── Add WireGuard peer modal ────────────────────────────────── */
function showAddPeerModal(ifname) {
  showModal(
    `Add Peer — ${ifname}`,
    `<div class="form-group">
       <label class="form-label">Peer Public Key</label>
       <input class="form-input" id="m-peer-key" placeholder="base64 public key">
     </div>
     <div class="form-group">
       <label class="form-label">Allowed IPs (comma separated)</label>
       <input class="form-input" id="m-peer-ips" placeholder="10.0.0.2/32, 192.168.2.0/24">
     </div>
     <div class="form-group">
       <label class="form-label">Endpoint (optional, host:port)</label>
       <input class="form-input" id="m-peer-ep" placeholder="peer.example.com:51820">
     </div>
     <div class="form-group">
       <label class="form-label">Keepalive (seconds, 0 = disabled)</label>
       <input class="form-input" id="m-peer-ka" type="number" value="25">
     </div>`,
    async () => {
      const body = {
        interface:    ifname,
        public_key:   document.getElementById('m-peer-key').value.trim(),
        allowed_ips:  document.getElementById('m-peer-ips').value.split(',').map(s => s.trim()).filter(Boolean),
        endpoint:     document.getElementById('m-peer-ep').value.trim() || undefined,
        keepalive:    parseInt(document.getElementById('m-peer-ka').value) || 0,
      };
      if (!body.public_key) throw new Error('Public key is required');
      if (!body.allowed_ips.length) throw new Error('At least one allowed IP required');
      await POST(`/vpn/wireguard/${ifname}/peers`, body);
      toast('Peer added');
      loadVPN();
    },
    { submitLabel: 'Add Peer' }
  );
}

/* ── OpenVPN modal ───────────────────────────────────────────── */
function showOpenVPNModal() {
  showModal(
    'Add OpenVPN Configuration',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:12px">
       Requires: <code>apk add openvpn-openssl</code>
     </p>
     <div class="form-group">
       <label class="form-label">Tunnel Name</label>
       <input class="form-input" id="m-ovpn-name" placeholder="vpn0">
     </div>
     <div class="form-group">
       <label class="form-label">Mode</label>
       <select class="form-select" id="m-ovpn-mode">
         <option value="client">Client</option>
         <option value="server">Server</option>
       </select>
     </div>
     <div class="form-group">
       <label class="form-label">Remote Server</label>
       <input class="form-input" id="m-ovpn-remote" placeholder="vpn.example.com">
     </div>
     <div class="form-group">
       <label class="form-label">Port</label>
       <input class="form-input" id="m-ovpn-port" type="number" value="1194">
     </div>
     <div class="form-group">
       <label class="form-label">Protocol</label>
       <select class="form-select" id="m-ovpn-proto">
         <option value="udp">UDP</option>
         <option value="tcp">TCP</option>
       </select>
     </div>`,
    async () => {
      const body = {
        name:   document.getElementById('m-ovpn-name').value.trim(),
        mode:   document.getElementById('m-ovpn-mode').value,
        remote: document.getElementById('m-ovpn-remote').value.trim(),
        port:   parseInt(document.getElementById('m-ovpn-port').value) || 1194,
        proto:  document.getElementById('m-ovpn-proto').value,
      };
      if (!body.name) throw new Error('Tunnel name required');
      await POST('/vpn/openvpn', body);
      toast(`OpenVPN ${body.name} added`);
      loadVPN();
    },
    { submitLabel: 'Add OpenVPN' }
  );
}

/* ── IPSec modal ─────────────────────────────────────────────── */
function showIPSecModal() {
  showModal(
    'Add IPSec / IKEv2 Tunnel',
    `<p style="color:var(--text-muted);font-size:0.8rem;margin-bottom:12px">
       Requires: <code>apk add strongswan</code>
     </p>
     <div class="form-group">
       <label class="form-label">Connection Name</label>
       <input class="form-input" id="m-ipsec-name" placeholder="site-to-site">
     </div>
     <div class="form-group">
       <label class="form-label">Remote Address</label>
       <input class="form-input" id="m-ipsec-remote" placeholder="203.0.113.1">
     </div>
     <div class="form-group">
       <label class="form-label">Local Subnet</label>
       <input class="form-input" id="m-ipsec-local" placeholder="192.168.1.0/24">
     </div>
     <div class="form-group">
       <label class="form-label">Remote Subnet</label>
       <input class="form-input" id="m-ipsec-rnet" placeholder="192.168.2.0/24">
     </div>`,
    async () => {
      const body = {
        name:          document.getElementById('m-ipsec-name').value.trim(),
        remote_addr:   document.getElementById('m-ipsec-remote').value.trim(),
        local_subnet:  document.getElementById('m-ipsec-local').value.trim(),
        remote_subnet: document.getElementById('m-ipsec-rnet').value.trim(),
      };
      if (!body.name || !body.remote_addr) throw new Error('Name and remote address required');
      await POST('/vpn/ipsec', body);
      toast(`IPSec tunnel ${body.name} added`);
      loadVPN();
    },
    { submitLabel: 'Add IPSec' }
  );
}

/* ── Delete tunnel ───────────────────────────────────────────── */
async function deleteTunnel(name, type) {
  confirmDialog(`Delete ${type?.toUpperCase()} tunnel "${name}"?`, async () => {
    await DELETE(`/vpn/tunnels/${name}`);
    toast(`Tunnel ${name} deleted`);
    loadVPN();
  }, 'Delete Tunnel');
}
