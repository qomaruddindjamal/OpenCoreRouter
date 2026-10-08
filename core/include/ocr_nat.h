/**
 * ocr_nat.h - Open Core Router NAT Engine
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 */

#ifndef OCR_NAT_H
#define OCR_NAT_H

#include "ocr_types.h"

/* NAT table */
typedef struct ocr_nat_table {
    ocr_nat_rule_t  **rules;
    int              rule_count;
    int              rule_cap;
    uint32_t         next_id;
} ocr_nat_table_t;

/* NAT table operations */
ocr_nat_table_t *ocr_nat_table_create(void);
void              ocr_nat_table_destroy(ocr_nat_table_t *tbl);
ocr_err_t         ocr_nat_table_add(ocr_nat_table_t *tbl, ocr_nat_rule_t *rule);
ocr_err_t         ocr_nat_table_del(ocr_nat_table_t *tbl, uint32_t id);
ocr_nat_rule_t   *ocr_nat_table_get(ocr_nat_table_t *tbl, uint32_t id);
ocr_err_t         ocr_nat_table_apply(ocr_nat_table_t *tbl);
ocr_err_t         ocr_nat_table_flush(ocr_nat_table_t *tbl);

/* Masquerade shortcut */
ocr_err_t         ocr_nat_masquerade(const char *out_iface,
                                      const ocr_prefix_t *src_net);
ocr_err_t         ocr_nat_masquerade_del(const char *out_iface);

#endif /* OCR_NAT_H */
