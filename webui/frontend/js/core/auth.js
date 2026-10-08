/**
 * core/auth.js — Authentication (JWT Login/Logout)
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Talks to POST /api/v1/auth/login, stores JWT in localStorage,
 * exposes getToken() used by api.js.
 */

'use strict';

import { POST } from './api.js';
import { toastError } from './toast.js';

const TOKEN_KEY    = 'ocr_token';
const USERNAME_KEY = 'ocr_user';

/** In-memory token reference (avoids repeated localStorage reads) */
window._ocrToken = localStorage.getItem(TOKEN_KEY);

/** Retrieve the current JWT */
export function getToken() {
  return window._ocrToken;
}

/** Retrieve the current username */
export function getUsername() {
  return localStorage.getItem(USERNAME_KEY) ?? 'admin';
}

/** Returns true if a valid token is stored */
export function isLoggedIn() {
  return Boolean(window._ocrToken);
}

/**
 * Perform login
 * @param {string} username
 * @param {string} password
 * @returns {Promise<string>} JWT token
 */
export async function login(username, password) {
  const data = await POST('/auth/login', { username, password });

  window._ocrToken = data.token;
  localStorage.setItem(TOKEN_KEY,    data.token);
  localStorage.setItem(USERNAME_KEY, username);

  // Update sidebar username display
  const el = document.getElementById('sidebar-username');
  if (el) el.textContent = username;

  return data.token;
}

/**
 * Logout — clear token and show login overlay
 */
export function logout() {
  window._ocrToken = null;
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(USERNAME_KEY);

  // Close WebSocket if open
  if (window._ocrWs) {
    window._ocrWs.close();
    window._ocrWs = null;
  }

  document.getElementById('login-overlay')?.classList.remove('hidden');
}

/**
 * Initialize the login form
 * Must be called once on page load.
 * @param {Function} onSuccess - callback after successful login
 */
export function initAuthForm(onSuccess) {
  const form    = document.getElementById('login-form');
  const errEl   = document.getElementById('login-error');
  const btnEl   = document.getElementById('btn-login');
  const logoutBtn = document.getElementById('btn-logout');

  form?.addEventListener('submit', async (e) => {
    e.preventDefault();
    errEl.textContent = '';
    btnEl.disabled = true;
    btnEl.textContent = 'Signing in...';

    const user = document.getElementById('login-user')?.value.trim();
    const pass = document.getElementById('login-pass')?.value;

    try {
      await login(user, pass);
      document.getElementById('login-overlay')?.classList.add('hidden');
      onSuccess?.();
    } catch (err) {
      errEl.textContent = 'Invalid username or password';
    } finally {
      btnEl.disabled = false;
      btnEl.textContent = 'Sign In';
    }
  });

  logoutBtn?.addEventListener('click', logout);
}
