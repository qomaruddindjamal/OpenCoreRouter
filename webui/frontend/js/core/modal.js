/**
 * core/modal.js — Reusable Modal Dialog
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

'use strict';

import { toastError } from './toast.js';

const overlay  = () => document.getElementById('modal-overlay');
const titleEl  = () => document.getElementById('modal-title');
const bodyEl   = () => document.getElementById('modal-body');
const submitEl = () => document.getElementById('modal-submit');
const cancelEl = () => document.getElementById('modal-cancel');
const closeEl  = () => document.getElementById('modal-close');

/** Initialize modal event listeners (called once on app init) */
export function initModal() {
  closeEl()?.addEventListener('click', hide);
  cancelEl()?.addEventListener('click', hide);
  overlay()?.addEventListener('click', (e) => {
    if (e.target === overlay()) hide();
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && overlay()?.classList.contains('active')) hide();
  });
}

/**
 * Show the modal dialog
 * @param {string}   title       - Modal header title
 * @param {string}   bodyHTML    - Inner HTML for modal body
 * @param {Function} onConfirm   - Async function called on submit
 * @param {object}   [opts]
 * @param {string}   [opts.submitLabel='Confirm']
 * @param {string}   [opts.submitClass='btn-primary']
 */
export function show(title, bodyHTML, onConfirm, opts = {}) {
  const {
    submitLabel = 'Confirm',
    submitClass = 'btn-primary',
  } = opts;

  titleEl().textContent = title;
  bodyEl().innerHTML    = bodyHTML;

  const btn = submitEl();
  btn.textContent = submitLabel;
  btn.className   = `btn-primary ${submitClass}`;

  // Remove old handler
  const newBtn = btn.cloneNode(true);
  btn.parentNode.replaceChild(newBtn, btn);

  newBtn.addEventListener('click', async () => {
    newBtn.disabled     = true;
    newBtn.textContent  = 'Processing...';
    try {
      await onConfirm();
      hide();
    } catch (err) {
      toastError(err.message);
    } finally {
      newBtn.disabled    = false;
      newBtn.textContent = submitLabel;
    }
  });

  overlay().classList.add('active');

  // Focus first input
  setTimeout(() => bodyEl().querySelector('input, select, textarea')?.focus(), 50);
}

/** Hide the modal */
export function hide() {
  overlay()?.classList.remove('active');
}

/**
 * Confirm-only dialog (no form body)
 * @param {string}   message
 * @param {Function} onConfirm
 * @param {string}   [title='Confirm']
 */
export function confirm(message, onConfirm, title = 'Confirm') {
  show(title, `<p style="color:var(--text-secondary)">${message}</p>`, onConfirm, {
    submitLabel: 'Yes, proceed',
    submitClass: 'btn-danger',
  });
}
