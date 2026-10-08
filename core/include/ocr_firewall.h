/**
 * ocr_firewall.h - Open Core Router Firewall Engine
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 *
 * Uses nftables as backend for packet filtering.
 */

#ifndef OCR_FIREWALL_H
#define OCR_FIREWALL_H

#include "ocr_types.h"
#include <pthread.h>

/* Connection tracking state flags */
#define OCR_CONNTRACK_NEW         (1 << 0)
#define OCR_CONNTRACK_ESTABLISHED (1 << 1)
#define OCR_CONNTRACK_RELATED     (1 << 2)
#define OCR_CONNTRACK_INVALID     (1 << 3)

/* Firewall chain */
typedef struct ocr_fw_chain {
    char             name[64];
    ocr_fw_direction_t direction;
    ocr_fw_action_t  default_action;
    ocr_fw_rule_t    **rules;
    int              rule_count;
    int              rule_cap;
} ocr_fw_chain_t;

/* Firewall engine */
typedef struct ocr_firewall {
    ocr_fw_chain_t  *chains[OCR_MAX_INTERFACES * 3];  /* in/out/fwd per iface */
    int              chain_count;
    pthread_rwlock_t lock;
    bool             enabled;
    bool             default_accept;  /* default policy */
} ocr_firewall_t;

/* Firewall operations */
ocr_firewall_t  *ocr_fw_create(void);
void              ocr_fw_destroy(ocr_firewall_t *fw);
ocr_err_t         ocr_fw_enable(ocr_firewall_t *fw);
ocr_err_t         ocr_fw_disable(ocr_firewall_t *fw);
ocr_err_t         ocr_fw_flush(ocr_firewall_t *fw);

/* Chain operations */
ocr_fw_chain_t  *ocr_fw_chain_create(ocr_firewall_t *fw, const char *name,
                                       ocr_fw_direction_t dir,
                                       ocr_fw_action_t default_action);
ocr_err_t         ocr_fw_chain_destroy(ocr_firewall_t *fw, const char *name);
ocr_fw_chain_t  *ocr_fw_chain_get(ocr_firewall_t *fw, const char *name);

/* Rule operations */
ocr_err_t         ocr_fw_rule_add(ocr_firewall_t *fw, const char *chain,
                                   ocr_fw_rule_t *rule);
ocr_err_t         ocr_fw_rule_del(ocr_firewall_t *fw, const char *chain,
                                   uint32_t rule_id);
ocr_err_t         ocr_fw_rule_update(ocr_firewall_t *fw, const char *chain,
                                      ocr_fw_rule_t *rule);
ocr_err_t         ocr_fw_rule_move(ocr_firewall_t *fw, const char *chain,
                                    uint32_t rule_id, int new_pos);

/* NAT operations */
ocr_err_t         ocr_nat_add_rule(ocr_firewall_t *fw, ocr_nat_rule_t *rule);
ocr_err_t         ocr_nat_del_rule(ocr_firewall_t *fw, uint32_t rule_id);
ocr_err_t         ocr_nat_flush(ocr_firewall_t *fw);

/* Apply rules to nftables */
ocr_err_t         ocr_fw_apply(ocr_firewall_t *fw);
ocr_err_t         ocr_fw_save(ocr_firewall_t *fw, const char *filepath);
ocr_err_t         ocr_fw_load(ocr_firewall_t *fw, const char *filepath);

/* Connection tracking */
ocr_err_t         ocr_conntrack_enable(void);
ocr_err_t         ocr_conntrack_flush(void);
int               ocr_conntrack_count(void);

#endif /* OCR_FIREWALL_H */
