/**
 * ocr_interface.h - Open Core Router Interface Management
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 */

#ifndef OCR_INTERFACE_H
#define OCR_INTERFACE_H

#include "ocr_types.h"
#include <pthread.h>

/* Interface manager */
typedef struct ocr_ifmgr {
    ocr_interface_t *interfaces[OCR_MAX_INTERFACES];
    int              count;
    pthread_rwlock_t lock;

    /* Callbacks */
    void (*on_iface_up)(struct ocr_ifmgr *mgr, ocr_interface_t *iface);
    void (*on_iface_down)(struct ocr_ifmgr *mgr, ocr_interface_t *iface);
    void (*on_addr_add)(struct ocr_ifmgr *mgr, ocr_interface_t *iface,
                        const ocr_prefix_t *prefix);
    void (*on_addr_del)(struct ocr_ifmgr *mgr, ocr_interface_t *iface,
                        const ocr_prefix_t *prefix);
} ocr_ifmgr_t;

/* Interface manager operations */
ocr_ifmgr_t     *ocr_ifmgr_create(void);
void              ocr_ifmgr_destroy(ocr_ifmgr_t *mgr);

/* Sync interfaces from kernel (via netlink) */
ocr_err_t         ocr_ifmgr_sync(ocr_ifmgr_t *mgr);

/* Interface CRUD */
ocr_interface_t  *ocr_ifmgr_get(ocr_ifmgr_t *mgr, const char *name);
ocr_interface_t  *ocr_ifmgr_get_by_index(ocr_ifmgr_t *mgr, int ifindex);
ocr_err_t         ocr_ifmgr_add(ocr_ifmgr_t *mgr, ocr_interface_t *iface);
ocr_err_t         ocr_ifmgr_del(ocr_ifmgr_t *mgr, const char *name);

/* Interface configuration */
ocr_err_t         ocr_iface_set_up(const char *ifname);
ocr_err_t         ocr_iface_set_down(const char *ifname);
ocr_err_t         ocr_iface_set_mtu(const char *ifname, uint32_t mtu);
ocr_err_t         ocr_iface_set_promisc(const char *ifname, bool enable);

/* Address management */
ocr_err_t         ocr_iface_add_addr(const char *ifname,
                                      const ocr_prefix_t *prefix);
ocr_err_t         ocr_iface_del_addr(const char *ifname,
                                      const ocr_prefix_t *prefix);

/* VLAN management */
ocr_err_t         ocr_vlan_create(const char *parent, uint16_t vlan_id,
                                   char *out_name, size_t out_len);
ocr_err_t         ocr_vlan_destroy(const char *vlan_ifname);

/* Bridge management */
ocr_err_t         ocr_bridge_create(const char *name);
ocr_err_t         ocr_bridge_destroy(const char *name);
ocr_err_t         ocr_bridge_add_port(const char *bridge, const char *port);
ocr_err_t         ocr_bridge_del_port(const char *bridge, const char *port);

/* Statistics update */
ocr_err_t         ocr_iface_update_stats(ocr_interface_t *iface);

/* Walk all interfaces */
typedef void (*ocr_iface_walk_fn)(ocr_interface_t *iface, void *arg);
void              ocr_ifmgr_walk(ocr_ifmgr_t *mgr, ocr_iface_walk_fn fn,
                                  void *arg);

#endif /* OCR_INTERFACE_H */
