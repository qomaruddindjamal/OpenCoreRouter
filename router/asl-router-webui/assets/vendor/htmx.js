// htmx 1.9.10 (unminified stub for bare-metal / embedded UI deployment)
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
