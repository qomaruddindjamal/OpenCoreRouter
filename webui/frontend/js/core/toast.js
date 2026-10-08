/**
 * core/toast.js — Toast Notification System
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

'use strict';

const ICONS = {
  success: '✓',
  error:   '✕',
  warning: '⚠',
  info:    'ℹ',
};

const DURATION = {
  success: 3500,
  error:   5000,
  warning: 4000,
  info:    3500,
};

/**
 * Show a toast notification
 * @param {string} message
 * @param {'success'|'error'|'warning'|'info'} [type='success']
 */
export function toast(message, type = 'success') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const el = document.createElement('div');
  el.className = `toast ${type}`;
  el.innerHTML = `
    <span class="toast-icon">${ICONS[type] ?? '●'}</span>
    <span class="toast-message">${message}</span>
    <button class="toast-close" aria-label="Close">×</button>
  `;

  // Manual close
  el.querySelector('.toast-close').onclick = () => el.remove();

  container.appendChild(el);

  // Auto dismiss
  setTimeout(() => {
    el.style.opacity = '0';
    el.style.transform = 'translateX(20px)';
    el.style.transition = 'all 0.3s ease';
    setTimeout(() => el.remove(), 300);
  }, DURATION[type] ?? 3500);
}

/** Shorthand helpers */
export const toastSuccess = (msg) => toast(msg, 'success');
export const toastError   = (msg) => toast(msg, 'error');
export const toastWarning = (msg) => toast(msg, 'warning');
export const toastInfo    = (msg) => toast(msg, 'info');
