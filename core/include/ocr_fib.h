/**
 * ocr_fib.h - Open Core Router Forwarding Information Base
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 *
 * The FIB contains only the best routes selected from the RIB
 * and is optimized for fast forwarding lookup.
 * Uses a radix trie (Patricia tree) for efficient longest-prefix match.
 */

#ifndef OCR_FIB_H
#define OCR_FIB_H

#include "ocr_types.h"
#include <pthread.h>

/* FIB node (Patricia trie) */
typedef struct ocr_fib_node {
    ocr_prefix_t          prefix;
    uint32_t              bit;           /* Bit position to test */
    struct ocr_fib_node  *left;
    struct ocr_fib_node  *right;
    ocr_nexthop_t         nexthops[OCR_MAX_NEXTHOPS];
    int                   nexthop_count;
    bool                  has_data;
} ocr_fib_node_t;

/* FIB table */
typedef struct ocr_fib {
    uint32_t         table_id;
    ocr_af_t         family;
    ocr_fib_node_t  *root4;    /* IPv4 trie root */
    ocr_fib_node_t  *root6;    /* IPv6 trie root */
    uint32_t         count;
    pthread_rwlock_t lock;
} ocr_fib_t;

/* FIB operations */
ocr_fib_t    *ocr_fib_create(uint32_t table_id);
void          ocr_fib_destroy(ocr_fib_t *fib);

ocr_err_t     ocr_fib_add(ocr_fib_t *fib, const ocr_prefix_t *prefix,
                           const ocr_nexthop_t *nexthops, int count);
ocr_err_t     ocr_fib_del(ocr_fib_t *fib, const ocr_prefix_t *prefix);
ocr_err_t     ocr_fib_update(ocr_fib_t *fib, const ocr_prefix_t *prefix,
                              const ocr_nexthop_t *nexthops, int count);

/* Longest prefix match lookup */
int           ocr_fib_lookup(ocr_fib_t *fib, const ocr_prefix_t *addr,
                              ocr_nexthop_t *out_nexthop);

/* Exact match lookup */
int           ocr_fib_lookup_exact(ocr_fib_t *fib, const ocr_prefix_t *prefix,
                                    ocr_nexthop_t *out_nexthops, int max_nh);

/* Walk all FIB entries */
typedef void (*ocr_fib_walk_fn)(const ocr_prefix_t *prefix,
                                  const ocr_nexthop_t *nexthops,
                                  int count, void *arg);
void          ocr_fib_walk(ocr_fib_t *fib, ocr_fib_walk_fn fn, void *arg);

/* Sync FIB to kernel routing table */
ocr_err_t     ocr_fib_sync_kernel(ocr_fib_t *fib);
int           ocr_fib_count(ocr_fib_t *fib);
void          ocr_fib_flush(ocr_fib_t *fib);

#endif /* OCR_FIB_H */
