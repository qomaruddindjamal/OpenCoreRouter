/**
 * rib.c - Open Core Router Routing Information Base Implementation
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 */

#include "ocr_rib.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <arpa/inet.h>

/* Administrative distance defaults */
static const uint8_t ocr_proto_distance[] = {
    [OCR_PROTO_STATIC]    = 1,
    [OCR_PROTO_KERNEL]    = 0,
    [OCR_PROTO_CONNECTED] = 0,
    [OCR_PROTO_OSPF]      = 110,
    [OCR_PROTO_BGP]       = 200,
    [OCR_PROTO_RIP]       = 120,
    [OCR_PROTO_ISIS]      = 115,
};

/* ------------------------------------------------------------------ */
/* Internal helpers                                                     */
/* ------------------------------------------------------------------ */

static int prefix_cmp(const ocr_prefix_t *a, const ocr_prefix_t *b)
{
    if (a->family != b->family)
        return a->family - b->family;
    if (a->prefixlen != b->prefixlen)
        return a->prefixlen - b->prefixlen;
    if (a->family == OCR_AF_INET)
        return memcmp(&a->addr.v4, &b->addr.v4, 4);
    return memcmp(&a->addr.v6, &b->addr.v6, 16);
}

static ocr_rib_entry_t *rib_entry_alloc(const ocr_prefix_t *prefix)
{
    ocr_rib_entry_t *e = calloc(1, sizeof(*e));
    if (!e) return NULL;
    e->prefix    = *prefix;
    e->route_cap = 4;
    e->routes    = malloc(sizeof(ocr_route_t *) * e->route_cap);
    if (!e->routes) {
        free(e);
        return NULL;
    }
    return e;
}

static void rib_entry_free(ocr_rib_entry_t *e)
{
    if (!e) return;
    for (int i = 0; i < e->route_count; i++)
        free(e->routes[i]);
    free(e->routes);
    free(e);
}

/* Binary tree insert */
static ocr_rib_entry_t *rib_tree_insert(ocr_rib_entry_t *root,
                                          ocr_rib_entry_t *node)
{
    if (!root) return node;
    int cmp = prefix_cmp(&node->prefix, &root->prefix);
    if (cmp < 0) {
        root->left = rib_tree_insert(root->left, node);
        root->left->parent = root;
    } else if (cmp > 0) {
        root->right = rib_tree_insert(root->right, node);
        root->right->parent = root;
    }
    /* cmp == 0: prefix already exists, caller handles this */
    return root;
}

static ocr_rib_entry_t *rib_tree_find(ocr_rib_entry_t *root,
                                       const ocr_prefix_t *prefix)
{
    while (root) {
        int cmp = prefix_cmp(prefix, &root->prefix);
        if (cmp == 0) return root;
        root = (cmp < 0) ? root->left : root->right;
    }
    return NULL;
}

/* Select best route from entry's route list */
static ocr_route_t *rib_select_best(ocr_rib_entry_t *e)
{
    ocr_route_t *best = NULL;
    for (int i = 0; i < e->route_count; i++) {
        ocr_route_t *r = e->routes[i];
        if (!best) { best = r; continue; }
        /* Lower distance wins */
        if (r->distance < best->distance) { best = r; continue; }
        if (r->distance == best->distance && r->metric < best->metric)
            best = r;
    }
    return best;
}

/* Walk tree in-order */
static void rib_tree_walk(ocr_rib_entry_t *node, ocr_rib_t *rib,
                           ocr_rib_walk_fn fn, void *arg)
{
    if (!node) return;
    rib_tree_walk(node->left, rib, fn, arg);
    for (int i = 0; i < node->route_count; i++) {
        if (!fn(rib, node->routes[i], arg)) return;
    }
    rib_tree_walk(node->right, rib, fn, arg);
}

/* Free entire tree */
static void rib_tree_free(ocr_rib_entry_t *node)
{
    if (!node) return;
    rib_tree_free(node->left);
    rib_tree_free(node->right);
    rib_entry_free(node);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

ocr_rib_t *ocr_rib_create(uint32_t table_id, ocr_af_t family)
{
    ocr_rib_t *rib = calloc(1, sizeof(*rib));
    if (!rib) return NULL;

    rib->table_id = table_id;
    rib->family   = family;

    if (pthread_rwlock_init(&rib->lock, NULL) != 0) {
        free(rib);
        return NULL;
    }

    return rib;
}

void ocr_rib_destroy(ocr_rib_t *rib)
{
    if (!rib) return;
    pthread_rwlock_wrlock(&rib->lock);
    rib_tree_free(rib->root);
    pthread_rwlock_unlock(&rib->lock);
    pthread_rwlock_destroy(&rib->lock);
    free(rib);
}

ocr_err_t ocr_rib_add_route(ocr_rib_t *rib, ocr_route_t *route)
{
    if (!rib || !route) return OCR_ERR_INVAL;

    /* Set default distance if not set */
    if (route->distance == 0 &&
        route->protocol < (int)OCR_ARRAY_SIZE(ocr_proto_distance))
        route->distance = ocr_proto_distance[route->protocol];

    /* Set installation time */
    route->installed_time = (uint64_t)time(NULL);

    pthread_rwlock_wrlock(&rib->lock);

    ocr_rib_entry_t *e = rib_tree_find(rib->root, &route->prefix);
    if (!e) {
        e = rib_entry_alloc(&route->prefix);
        if (!e) {
            pthread_rwlock_unlock(&rib->lock);
            return OCR_ERR_NOMEM;
        }
        rib->root = rib_tree_insert(rib->root, e);
        rib->entry_count++;
    }

    /* Check duplicate protocol */
    for (int i = 0; i < e->route_count; i++) {
        if (e->routes[i]->protocol == route->protocol) {
            /* Replace existing */
            ocr_route_t *old = e->routes[i];
            e->routes[i] = route;
            /* Update best */
            e->best = rib_select_best(e);
            /* Trigger callback */
            if (rib->on_route_change)
                rib->on_route_change(rib, old, route);
            free(old);
            pthread_rwlock_unlock(&rib->lock);
            return OCR_OK;
        }
    }

    /* Add new route */
    if (e->route_count >= e->route_cap) {
        int new_cap = e->route_cap * 2;
        ocr_route_t **new_routes = realloc(e->routes,
                                            sizeof(ocr_route_t *) * new_cap);
        if (!new_routes) {
            pthread_rwlock_unlock(&rib->lock);
            return OCR_ERR_NOMEM;
        }
        e->routes    = new_routes;
        e->route_cap = new_cap;
    }

    e->routes[e->route_count++] = route;
    e->best = rib_select_best(e);
    rib->route_count++;

    if (rib->on_route_add)
        rib->on_route_add(rib, route);

    pthread_rwlock_unlock(&rib->lock);
    return OCR_OK;
}

ocr_err_t ocr_rib_del_route(ocr_rib_t *rib, const ocr_prefix_t *prefix,
                              ocr_proto_t proto)
{
    if (!rib || !prefix) return OCR_ERR_INVAL;

    pthread_rwlock_wrlock(&rib->lock);

    ocr_rib_entry_t *e = rib_tree_find(rib->root, prefix);
    if (!e) {
        pthread_rwlock_unlock(&rib->lock);
        return OCR_ERR_NOTFOUND;
    }

    int found = -1;
    for (int i = 0; i < e->route_count; i++) {
        if (e->routes[i]->protocol == proto) {
            found = i;
            break;
        }
    }

    if (found < 0) {
        pthread_rwlock_unlock(&rib->lock);
        return OCR_ERR_NOTFOUND;
    }

    ocr_route_t *deleted = e->routes[found];
    /* Shift remaining routes */
    for (int i = found; i < e->route_count - 1; i++)
        e->routes[i] = e->routes[i + 1];
    e->route_count--;
    rib->route_count--;

    e->best = rib_select_best(e);

    if (rib->on_route_del)
        rib->on_route_del(rib, deleted);

    free(deleted);
    pthread_rwlock_unlock(&rib->lock);
    return OCR_OK;
}

ocr_route_t *ocr_rib_lookup(ocr_rib_t *rib, const ocr_prefix_t *prefix)
{
    if (!rib || !prefix) return NULL;
    pthread_rwlock_rdlock(&rib->lock);
    ocr_rib_entry_t *e = rib_tree_find(rib->root, prefix);
    ocr_route_t *r = e ? e->best : NULL;
    pthread_rwlock_unlock(&rib->lock);
    return r;
}

ocr_route_t *ocr_rib_lookup_exact(ocr_rib_t *rib, const ocr_prefix_t *prefix)
{
    return ocr_rib_lookup(rib, prefix);
}

ocr_route_t *ocr_rib_lookup_best(ocr_rib_t *rib, const ocr_prefix_t *prefix)
{
    return ocr_rib_lookup(rib, prefix);
}

void ocr_rib_walk(ocr_rib_t *rib, ocr_rib_walk_fn fn, void *arg)
{
    if (!rib || !fn) return;
    pthread_rwlock_rdlock(&rib->lock);
    rib_tree_walk(rib->root, rib, fn, arg);
    pthread_rwlock_unlock(&rib->lock);
}

int ocr_rib_route_count(ocr_rib_t *rib)
{
    return rib ? (int)rib->route_count : 0;
}

void ocr_rib_flush(ocr_rib_t *rib)
{
    if (!rib) return;
    pthread_rwlock_wrlock(&rib->lock);
    rib_tree_free(rib->root);
    rib->root        = NULL;
    rib->entry_count = 0;
    rib->route_count = 0;
    pthread_rwlock_unlock(&rib->lock);
}

void ocr_rib_flush_proto(ocr_rib_t *rib, ocr_proto_t proto)
{
    if (!rib) return;
    /* Simple approach: collect and delete */
    /* A production impl would do this in one pass */
    typedef struct { ocr_prefix_t prefix; } item_t;
    item_t items[1024];
    int count = 0;

    pthread_rwlock_rdlock(&rib->lock);
    /* Walk and collect matching prefixes */
    /* ... simplified: just call flush for now */
    pthread_rwlock_unlock(&rib->lock);

    OCR_UNUSED(proto);
    OCR_UNUSED(items);
    OCR_UNUSED(count);
}
