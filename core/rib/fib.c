/**
 * fib.c - Open Core Router Forwarding Information Base Implementation
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 *
 * Patricia trie (radix tree) for fast O(log n) longest-prefix match.
 */

#include "ocr_fib.h"
#include <stdlib.h>
#include <string.h>
#include <arpa/inet.h>

/* ------------------------------------------------------------------ */
/* Bit operations                                                        */
/* ------------------------------------------------------------------ */

static inline uint8_t prefix_bit(const ocr_prefix_t *p, uint32_t bit)
{
    if (p->family == OCR_AF_INET) {
        uint32_t addr = ntohl(p->addr.v4.s_addr);
        return (addr >> (31 - bit)) & 1;
    } else {
        uint8_t *bytes = (uint8_t *)&p->addr.v6;
        return (bytes[bit / 8] >> (7 - (bit % 8))) & 1;
    }
}

static inline uint32_t prefix_max_bits(const ocr_prefix_t *p)
{
    return (p->family == OCR_AF_INET) ? 32 : 128;
}

static bool prefix_equal(const ocr_prefix_t *a, const ocr_prefix_t *b)
{
    if (a->family != b->family || a->prefixlen != b->prefixlen)
        return false;
    if (a->family == OCR_AF_INET)
        return a->addr.v4.s_addr == b->addr.v4.s_addr;
    return memcmp(&a->addr.v6, &b->addr.v6, 16) == 0;
}

/* Check if addr is covered by prefix */
static bool prefix_contains(const ocr_prefix_t *prefix,
                              const ocr_prefix_t *addr)
{
    if (prefix->family != addr->family) return false;

    if (prefix->family == OCR_AF_INET) {
        uint32_t mask = (prefix->prefixlen == 0) ? 0 :
                        htonl(~((1u << (32 - prefix->prefixlen)) - 1));
        return (addr->addr.v4.s_addr & mask) == (prefix->addr.v4.s_addr & mask);
    } else {
        uint8_t *paddr = (uint8_t *)&prefix->addr.v6;
        uint8_t *saddr = (uint8_t *)&addr->addr.v6;
        int bytes  = prefix->prefixlen / 8;
        int bits   = prefix->prefixlen % 8;

        if (memcmp(paddr, saddr, bytes) != 0)
            return false;
        if (bits == 0)
            return true;
        uint8_t mask = 0xFF << (8 - bits);
        return (paddr[bytes] & mask) == (saddr[bytes] & mask);
    }
}

/* ------------------------------------------------------------------ */
/* Patricia trie operations                                              */
/* ------------------------------------------------------------------ */

static ocr_fib_node_t *fib_node_alloc(const ocr_prefix_t *prefix, uint32_t bit)
{
    ocr_fib_node_t *n = calloc(1, sizeof(*n));
    if (!n) return NULL;
    if (prefix) n->prefix = *prefix;
    n->bit = bit;
    return n;
}

static void fib_tree_free(ocr_fib_node_t *node)
{
    if (!node) return;
    fib_tree_free(node->left);
    fib_tree_free(node->right);
    free(node);
}

/* Insert into Patricia trie */
static ocr_fib_node_t *fib_tree_insert(ocr_fib_node_t *root,
                                         const ocr_prefix_t *prefix,
                                         const ocr_nexthop_t *nexthops,
                                         int nh_count)
{
    if (!root) {
        ocr_fib_node_t *n = fib_node_alloc(prefix, prefix->prefixlen);
        if (!n) return NULL;
        n->has_data = true;
        int cnt = OCR_MIN(nh_count, OCR_MAX_NEXTHOPS);
        memcpy(n->nexthops, nexthops, sizeof(ocr_nexthop_t) * cnt);
        n->nexthop_count = cnt;
        n->left  = n;
        n->right = n;
        return n;
    }

    /* Find bit difference */
    uint32_t max_bits = prefix_max_bits(prefix);
    uint32_t bit = 0;
    while (bit < max_bits &&
           prefix_bit(prefix, bit) == prefix_bit(&root->prefix, bit))
        bit++;

    if (bit >= root->bit) {
        /* Insert at or below current node */
        ocr_fib_node_t *n = fib_node_alloc(prefix, prefix->prefixlen);
        if (!n) return root;
        n->has_data = true;
        int cnt = OCR_MIN(nh_count, OCR_MAX_NEXTHOPS);
        memcpy(n->nexthops, nexthops, sizeof(ocr_nexthop_t) * cnt);
        n->nexthop_count = cnt;
        n->left = n->right = n;
        return n;
    }

    /* Walk tree */
    if (prefix_bit(prefix, root->bit))
        root->right = fib_tree_insert(root->right, prefix, nexthops, nh_count);
    else
        root->left  = fib_tree_insert(root->left, prefix, nexthops, nh_count);

    return root;
}

/* Longest prefix match lookup */
static ocr_fib_node_t *fib_lpm(ocr_fib_node_t *root, const ocr_prefix_t *addr)
{
    ocr_fib_node_t *best = NULL;
    ocr_fib_node_t *cur  = root;

    while (cur) {
        if (cur->has_data && prefix_contains(&cur->prefix, addr)) {
            if (!best || cur->prefix.prefixlen > best->prefix.prefixlen)
                best = cur;
        }
        if (cur->left == cur || cur->right == cur) break;
        if (prefix_bit(addr, cur->bit))
            cur = cur->right;
        else
            cur = cur->left;
    }

    return best;
}

static void fib_walk_tree(ocr_fib_node_t *node, ocr_fib_walk_fn fn, void *arg)
{
    if (!node) return;
    if (node->has_data)
        fn(&node->prefix, node->nexthops, node->nexthop_count, arg);
    if (node->left && node->left != node)
        fib_walk_tree(node->left, fn, arg);
    if (node->right && node->right != node)
        fib_walk_tree(node->right, fn, arg);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

ocr_fib_t *ocr_fib_create(uint32_t table_id)
{
    ocr_fib_t *fib = calloc(1, sizeof(*fib));
    if (!fib) return NULL;
    fib->table_id = table_id;
    if (pthread_rwlock_init(&fib->lock, NULL) != 0) {
        free(fib);
        return NULL;
    }
    return fib;
}

void ocr_fib_destroy(ocr_fib_t *fib)
{
    if (!fib) return;
    pthread_rwlock_wrlock(&fib->lock);
    fib_tree_free(fib->root4);
    fib_tree_free(fib->root6);
    pthread_rwlock_unlock(&fib->lock);
    pthread_rwlock_destroy(&fib->lock);
    free(fib);
}

ocr_err_t ocr_fib_add(ocr_fib_t *fib, const ocr_prefix_t *prefix,
                       const ocr_nexthop_t *nexthops, int count)
{
    if (!fib || !prefix || !nexthops || count <= 0)
        return OCR_ERR_INVAL;

    pthread_rwlock_wrlock(&fib->lock);

    ocr_fib_node_t **root = (prefix->family == OCR_AF_INET) ?
                             &fib->root4 : &fib->root6;
    *root = fib_tree_insert(*root, prefix, nexthops, count);

    if (*root) fib->count++;

    pthread_rwlock_unlock(&fib->lock);
    return *root ? OCR_OK : OCR_ERR_NOMEM;
}

ocr_err_t ocr_fib_del(ocr_fib_t *fib, const ocr_prefix_t *prefix)
{
    if (!fib || !prefix) return OCR_ERR_INVAL;

    pthread_rwlock_wrlock(&fib->lock);

    /* Simplified: find and mark as no-data */
    ocr_fib_node_t *root = (prefix->family == OCR_AF_INET) ?
                            fib->root4 : fib->root6;
    ocr_fib_node_t *cur = root;
    while (cur) {
        if (prefix_equal(&cur->prefix, prefix)) {
            if (cur->has_data) {
                cur->has_data = false;
                cur->nexthop_count = 0;
                fib->count--;
            }
            break;
        }
        if (cur->left == cur || cur->right == cur) break;
        if (prefix_bit(prefix, cur->bit))
            cur = cur->right;
        else
            cur = cur->left;
    }

    pthread_rwlock_unlock(&fib->lock);
    return OCR_OK;
}

int ocr_fib_lookup(ocr_fib_t *fib, const ocr_prefix_t *addr,
                    ocr_nexthop_t *out_nexthop)
{
    if (!fib || !addr || !out_nexthop) return -1;

    pthread_rwlock_rdlock(&fib->lock);

    ocr_fib_node_t *root = (addr->family == OCR_AF_INET) ?
                            fib->root4 : fib->root6;
    ocr_fib_node_t *best = fib_lpm(root, addr);

    int ret = -1;
    if (best && best->nexthop_count > 0) {
        /* Simple: return first nexthop (or implement ECMP here) */
        *out_nexthop = best->nexthops[0];
        ret = best->nexthop_count;
    }

    pthread_rwlock_unlock(&fib->lock);
    return ret;
}

void ocr_fib_walk(ocr_fib_t *fib, ocr_fib_walk_fn fn, void *arg)
{
    if (!fib || !fn) return;
    pthread_rwlock_rdlock(&fib->lock);
    fib_walk_tree(fib->root4, fn, arg);
    fib_walk_tree(fib->root6, fn, arg);
    pthread_rwlock_unlock(&fib->lock);
}

int ocr_fib_count(ocr_fib_t *fib)
{
    return fib ? (int)fib->count : 0;
}

void ocr_fib_flush(ocr_fib_t *fib)
{
    if (!fib) return;
    pthread_rwlock_wrlock(&fib->lock);
    fib_tree_free(fib->root4);
    fib_tree_free(fib->root6);
    fib->root4 = fib->root6 = NULL;
    fib->count = 0;
    pthread_rwlock_unlock(&fib->lock);
}
