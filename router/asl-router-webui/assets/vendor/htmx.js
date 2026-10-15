// htmx 1.9.10 (unminified stub for bare-metal / embedded UI deployment)
// CATATAN Jujur 2026-10-15: ini BUKAN htmx upstream — hanya cangkang kosong
// tanpa mesin hx-*. Konsol Rinjani tidak lagi memuat file ini; rendering panel
// dilakukan driver sendiri di /js/status.js lewat XMLHttpRequest ke /api/v1/*.
// Aset tetap disajikan supaya URL warisan tidak 404, bukan sebagai dependensi.
(function (global, factory) {
    typeof exports === 'object' && typeof module !== 'undefined' ? module.exports = factory() :
    typeof define === 'function' && define.amd ? define(factory) :
    (global = typeof globalThis !== 'undefined' ? globalThis : global || self, global.htmx = factory());
})(this, (function () { 'use strict';
    var htmx = {
        version: "1.9.10",
        on: function(event, callback) {
            document.addEventListener(event, callback);
        },
        process: function(element) {
            // Process hx- attributes
        }
    };
    return htmx;
}));
