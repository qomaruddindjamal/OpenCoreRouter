/**
 * core/utils.js — Utility / Helper Functions
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

'use strict';

/**
 * Format bytes to human-readable string
 * @param {number} bytes
 * @param {number} [decimals=1]
 * @returns {string}  e.g. "12.3 MB"
 */
export function fmtBytes(bytes, decimals = 1) {
  if (!bytes || bytes === 0) return '0 B';
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(decimals)} ${sizes[i]}`;
}

/**
 * Format bits per second
 * @param {number} bps
 * @returns {string}  e.g. "5.2 Mbps"
 */
export function fmtBps(bps) {
  if (!bps || bps === 0) return '0 bps';
  const sizes = ['bps', 'Kbps', 'Mbps', 'Gbps', 'Tbps'];
  const i = Math.floor(Math.log(bps) / Math.log(1000));
  return `${(bps / Math.pow(1000, i)).toFixed(1)} ${sizes[i]}`;
}

/**
 * Format uptime in seconds to human-readable string
 * @param {number} seconds
 * @returns {string}  e.g. "3d 14h 20m"
 */
export function fmtUptime(seconds) {
  if (!seconds) return '--';
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;

  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  return `${m}m ${s}s`;
}

/**
 * Format a Unix timestamp (seconds) to locale string
 * @param {number} ts
 * @returns {string}
 */
export function fmtTimestamp(ts) {
  if (!ts) return '--';
  return new Date(ts * 1000).toLocaleString();
}

/**
 * Format a date string / ISO to locale
 * @param {string} dateStr
 * @returns {string}
 */
export function fmtDate(dateStr) {
  if (!dateStr) return '--';
  return new Date(dateStr).toLocaleString();
}

/**
 * Routing protocol number → name mapping (Linux kernel + FRRouting)
 * @param {number} proto
 * @returns {string}
 */
export function protoName(proto) {
  const map = {
    0:   'UNSPEC',
    1:   'STATIC',
    2:   'KERNEL',
    3:   'BOOT',
    4:   'CONNECTED',
    10:  'OSPF',
    11:  'BGP',
    12:  'RIP',
    13:  'ISIS',
    14:  'BABEL',
    42:  'EIGRP',
    186: 'STATIC', // FRR static
    187: 'OSPF',   // FRR OSPF
    188: 'ISIS',   // FRR ISIS
    189: 'BGP',    // FRR BGP
    190: 'PIM',    // FRR PIM
    191: 'RIPNG',
    192: 'NHRP',
    194: 'EIGRP',
    196: 'LDP',
    197: 'SHARP',
    198: 'PBR',
    199: 'ZSTATIC',
  };
  return map[proto] ?? `PROTO(${proto})`;
}

/**
 * Convert subnet mask (hex) to CIDR prefix length
 * @param {string} mask  - hex mask like "ffffff00"
 * @returns {number}
 */
export function maskToCidr(mask) {
  if (!mask) return 0;
  return mask.split('').reduce((acc, c) => {
    return acc + { 'f': 4, 'e': 3, 'c': 2, '8': 1 }[c.toLowerCase()] ?? 0;
  }, 0);
}

/**
 * Build a colored status badge HTML
 * @param {string} label
 * @param {'up'|'down'|'active'|'inactive'|'accept'|'drop'|'reject'|string} status
 * @returns {string} HTML string
 */
export function badge(label, status) {
  return `<span class="badge badge-${status.toLowerCase()}">${label}</span>`;
}

/**
 * Escape HTML special characters
 * @param {string} str
 * @returns {string}
 */
export function escHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

/**
 * Debounce a function
 * @param {Function} fn
 * @param {number}   wait  - ms
 * @returns {Function}
 */
export function debounce(fn, wait = 300) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), wait);
  };
}

/**
 * Sleep for ms milliseconds
 * @param {number} ms
 * @returns {Promise<void>}
 */
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Safely get nested property from object
 * @param {object} obj
 * @param {string} path  - dot-notation e.g. 'a.b.c'
 * @param {*}      def   - default value
 */
export function get(obj, path, def = null) {
  return path.split('.').reduce((o, k) => (o ?? {})[k], obj) ?? def;
}
