/**
 * ocr_qos.h - Open Core Router QoS Engine
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 *
 * Traffic shaping using Linux tc (traffic control) subsystem.
 * Implements HTB (Hierarchical Token Bucket) for bandwidth control.
 */

#ifndef OCR_QOS_H
#define OCR_QOS_H

#include "ocr_types.h"

/* QoS scheduler type */
typedef enum {
    OCR_QOS_SCHED_HTB   = 0,   /* Hierarchical Token Bucket */
    OCR_QOS_SCHED_PRIO,         /* Priority */
    OCR_QOS_SCHED_FQ_CODEL,     /* Fair Queue CoDel */
    OCR_QOS_SCHED_CAKE,          /* Common Applications Kept Enhanced */
} ocr_qos_sched_t;

/* QoS class */
typedef struct ocr_qos_class {
    uint32_t         id;
    char             name[64];
    char             iface[OCR_IFNAME_SIZE];
    uint32_t         parent_id;
    ocr_qos_sched_t  scheduler;
    uint64_t         rate;       /* guaranteed rate (bps) */
    uint64_t         ceil;       /* maximum rate (bps) */
    uint32_t         burst;      /* burst size (bytes) */
    uint32_t         cburst;     /* ceil burst size */
    uint32_t         priority;
    uint32_t         quantum;

    /* Statistics */
    uint64_t         sent_bytes;
    uint64_t         sent_packets;
    uint64_t         dropped;
} ocr_qos_class_t;

/* QoS filter (traffic classifier) */
typedef struct ocr_qos_filter {
    uint32_t         id;
    char             iface[OCR_IFNAME_SIZE];
    uint32_t         class_id;
    uint32_t         priority;
    ocr_prefix_t     src;
    ocr_prefix_t     dst;
    uint16_t         src_port;
    uint16_t         dst_port;
    uint8_t          protocol;
    uint8_t          dscp;
} ocr_qos_filter_t;

/* QoS manager */
typedef struct ocr_qos_mgr {
    ocr_qos_class_t  *classes[OCR_MAX_QOS_POLICIES];
    int               class_count;
    ocr_qos_filter_t *filters;
    int               filter_count;
    int               filter_cap;
} ocr_qos_mgr_t;

/* QoS operations */
ocr_qos_mgr_t   *ocr_qos_create(void);
void              ocr_qos_destroy(ocr_qos_mgr_t *mgr);

ocr_err_t         ocr_qos_class_add(ocr_qos_mgr_t *mgr, ocr_qos_class_t *cls);
ocr_err_t         ocr_qos_class_del(ocr_qos_mgr_t *mgr, uint32_t id);
ocr_err_t         ocr_qos_filter_add(ocr_qos_mgr_t *mgr,
                                      ocr_qos_filter_t *filter);
ocr_err_t         ocr_qos_filter_del(ocr_qos_mgr_t *mgr, uint32_t id);

/* Apply QoS to kernel (via tc commands) */
ocr_err_t         ocr_qos_apply(ocr_qos_mgr_t *mgr, const char *iface);
ocr_err_t         ocr_qos_reset(const char *iface);
ocr_err_t         ocr_qos_apply_cake(const char *iface, uint64_t bandwidth_bps);

/* DSCP marking */
ocr_err_t         ocr_qos_mark_dscp(const char *iface, ocr_prefix_t *src,
                                      ocr_prefix_t *dst, uint8_t dscp);

#endif /* OCR_QOS_H */
