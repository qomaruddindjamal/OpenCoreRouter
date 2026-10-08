/**
 * core/router.js — SPA Client-Side Router
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Handles page navigation in the sidebar, shows/hides .page divs,
 * and calls the page-specific loader on each navigation.
 */

'use strict';

/** Map of page name → loader function (registered by each page module) */
const pageLoaders = new Map();

/** Currently active page name */
let _currentPage = 'dashboard';

/** Register a page loader
 * @param {string}   pageName  - matches data-page attribute and #page-<name> id
 * @param {Function} loaderFn  - async function called on each navigation
 */
export function registerPage(pageName, loaderFn) {
  pageLoaders.set(pageName, loaderFn);
}

/** Get the currently active page name */
export function currentPage() {
  return _currentPage;
}

/** Navigate to a page
 * @param {string} page
 */
export async function navigate(page) {
  if (!pageLoaders.has(page)) {
    console.warn(`[router] No loader registered for page: "${page}"`);
    return;
  }

  // Deactivate all pages and nav items
  document.querySelectorAll('.page').forEach(el => el.classList.remove('active'));
  document.querySelectorAll('.nav-item[data-page]').forEach(el => el.classList.remove('active'));

  // Activate target page div
  document.getElementById(`page-${page}`)?.classList.add('active');

  // Activate nav item
  document.getElementById(`nav-${page}`)?.classList.add('active');

  // Update topbar title
  const title = page.charAt(0).toUpperCase() + page.slice(1).replace(/-/g, ' ');
  const titleEl = document.getElementById('page-title');
  if (titleEl) titleEl.textContent = title;

  _currentPage = page;

  // Load page data
  try {
    await pageLoaders.get(page)();
  } catch (err) {
    console.error(`[router] Page load error (${page}):`, err);
  }
}

/** Refresh the current page (re-run its loader) */
export async function refresh() {
  return navigate(_currentPage);
}

/** Initialize the sidebar navigation links */
export function initNav() {
  document.querySelectorAll('.nav-item[data-page]').forEach(el => {
    el.addEventListener('click', (e) => {
      e.preventDefault();
      navigate(el.dataset.page);
    });
  });

  // Mobile menu toggle
  document.getElementById('menu-toggle')?.addEventListener('click', () => {
    document.getElementById('sidebar')?.classList.toggle('open');
  });

  // Refresh button in topbar
  document.getElementById('btn-refresh')?.addEventListener('click', () => {
    refresh();
  });

  // Firewall tabs
  document.querySelectorAll('.tab[data-tab]').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
      document.querySelectorAll('.tab-content').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      document.getElementById(`tab-${tab.dataset.tab}`)?.classList.add('active');
    });
  });
}
