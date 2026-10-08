/**
 * app.js — Open Core Router Web UI Entry Point
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Bootstrap urutan:
 *  1. initModal()         — pasang event listener modal
 *  2. initAuthForm()      — pasang form login
 *  3. initNav()           — pasang sidebar nav links
 *  4. register all pages  — daftarkan page loaders ke router
 *  5. startClock()        — live clock di topbar
 *  6. Cek token:
 *     - Ada token  → sembunyikan login, startWebSocket(), navigate('dashboard')
 *     - Tidak ada  → tampilkan login overlay
 */

'use strict';

// ── Core modules ─────────────────────────────────────────────
import { initModal }         from './core/modal.js';
import { initAuthForm, isLoggedIn } from './core/auth.js';
import { initNav, navigate } from './core/router.js';
import { startWebSocket }    from './core/websocket.js';

// ── Page modules ──────────────────────────────────────────────
import { initDashboard }  from './pages/dashboard.js';
import { initInterfaces } from './pages/interfaces.js';
import { initRouting }    from './pages/routing.js';
import { initFirewall }   from './pages/firewall.js';
import { initVPN }        from './pages/vpn.js';
import { initDHCP }       from './pages/dhcp.js';
import { initDNS }        from './pages/dns.js';
import { initQoS }        from './pages/qos.js';
import { initSystem }     from './pages/system.js';

/* ── Live clock ───────────────────────────────────────────────── */
function startClock() {
  const el = document.getElementById('live-time');
  if (!el) return;
  const tick = () => { el.textContent = new Date().toLocaleTimeString(); };
  tick();
  setInterval(tick, 1000);
}

/* ── Bootstrap ───────────────────────────────────────────────── */
function bootstrap() {
  // 1. Modal system
  initModal();

  // 2. Sidebar nav links + mobile toggle
  initNav();

  // 3. Register all page loaders (order doesn't matter)
  initDashboard();
  initInterfaces();
  initRouting();
  initFirewall();
  initVPN();
  initDHCP();
  initDNS();
  initQoS();
  initSystem();

  // 4. Live clock
  startClock();

  // 5. Auth form — called with callback for post-login flow
  initAuthForm(() => {
    startWebSocket();
    navigate('dashboard');
  });

  // 6. Auto-login if token present
  if (isLoggedIn()) {
    document.getElementById('login-overlay')?.classList.add('hidden');
    startWebSocket();
    navigate('dashboard');
  }
}

// Run after DOM is ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', bootstrap);
} else {
  bootstrap();
}
