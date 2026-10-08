/**
 * ocr_rib.h - Open Core Router Routing Information Base
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 *
 * The RIB stores all routes received from protocols.
 * The best routes are selected and installed into the FIB.
 */

#ifndef OCR_RIB_H
#define OCR_RIB_H

#include "ocr_types.h"
#include <pthread.h>

/* RIB table entry */
typedef struct ocr_rib_entry {
    ocr_prefix_t         prefix;
    ocr_route_t          **routes;    /* Multiple routes for same prefix */
    int                  route_count;
    int                  route_cap;
    ocr_route_t          *best;       /* Selected best route */
    struct ocr_rib_entry *left;
    struct ocr_rib_entry *right;
    struct ocr_rib_entry *parent;
} ocr_rib_entry_t;

/* RIB table */
typedef struct ocr_rib {
    uint32_t             table_id;
    ocr_af_t             family;
    ocr_rib_entry_t      *root;
    uint32_t             entry_count;
    uint32_t             route_count;
    pthread_rwlock_t     lock;

    /* Callbacks */
    void (*on_route_add)(struct ocr_rib *rib, ocr_route_t *route);
    void (*on_route_del)(struct ocr_rib *rib, ocr_route_t *route);
    void (*on_route_change)(struct ocr_rib *rib, ocr_route_t *old_route,
                            ocr_route_t *new_route);
} ocr_rib_t;

/* RIB operations */
ocr_rib_t    *ocr_rib_create(uint32_t table_id, ocr_af_t family);
void          ocr_rib_destroy(ocr_rib_t *rib);

ocr_err_t     ocr_rib_add_route(ocr_rib_t *rib, ocr_route_t *route);
ocr_err_t     ocr_rib_del_route(ocr_rib_t *rib, const ocr_prefix_t *prefix,
                                 ocr_proto_t proto);
ocr_err_t     ocr_rib_replace_route(ocr_rib_t *rib, ocr_route_t *route);

ocr_route_t  *ocr_rib_lookup(ocr_rib_t *rib, const ocr_prefix_t *prefix);
ocr_route_t  *ocr_rib_lookup_exact(ocr_rib_t *rib, const ocr_prefix_t *prefix);
ocr_route_t  *ocr_rib_lookup_best(ocr_rib_t *rib, const ocr_prefix_t *prefix);

/* Walk callback: return true to continue, false to stop */
typedef bool (*ocr_rib_walk_fn)(ocr_rib_t *rib, ocr_route_t *route, void *arg);
void          ocr_rib_walk(ocr_rib_t *rib, ocr_rib_walk_fn fn, void *arg);

int           ocr_rib_route_count(ocr_rib_t *rib);
void          ocr_rib_flush(ocr_rib_t *rib);
void          ocr_rib_flush_proto(ocr_rib_t *rib, ocr_proto_t proto);

#endif /* OCR_RIB_H */
