/**
 * forwarding.c - Open Core Router L3 Forwarding Engine
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 */

#include "ocr_types.h"
#include "ocr_fib.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <netinet/ip.h>
#include <netinet/ip6.h>
#include <netinet/ether.h>

/* Forwarding statistics */
typedef struct ocr_fwd_stats {
    uint64_t  forwarded_pkts;
    uint64_t  forwarded_bytes;
    uint64_t  dropped_no_route;
    uint64_t  dropped_ttl;
    uint64_t  dropped_invalid;
    uint64_t  local_delivered;
} ocr_fwd_stats_t;

static ocr_fwd_stats_t g_fwd_stats;
static ocr_fib_t      *g_fib = NULL;

/* ------------------------------------------------------------------ */

void ocr_fwd_init(ocr_fib_t *fib)
{
    g_fib = fib;
    memset(&g_fwd_stats, 0, sizeof(g_fwd_stats));
}

/* Decrement TTL and check for expiry */
static bool fwd_check_ttl(struct iphdr *iph)
{
    if (iph->ttl <= 1) {
        g_fwd_stats.dropped_ttl++;
        return false;
    }
    iph->ttl--;
    return true;
}

/* Recalculate IPv4 checksum */
static uint16_t ip_checksum(void *vdata, size_t length)
{
    uint8_t *data = vdata;
    uint32_t acc = 0xffff;

    for (size_t i = 0; i + 1 < length; i += 2) {
        uint16_t word;
        memcpy(&word, data + i, 2);
        acc += ntohs(word);
        if (acc > 0xffff) acc -= 0xffff;
    }

    if (length & 1) {
        uint16_t word = 0;
        memcpy(&word, data + length - 1, 1);
        acc += ntohs(word);
        if (acc > 0xffff) acc -= 0xffff;
    }

    return htons(~acc);
}

/* Forward IPv4 packet */
int ocr_fwd_ipv4(uint8_t *pkt, uint32_t len)
{
    if (!g_fib || len < sizeof(struct iphdr)) {
        g_fwd_stats.dropped_invalid++;
        return -1;
    }

    struct iphdr *iph = (struct iphdr *)pkt;

    /* Validate IP version */
    if (iph->version != 4) {
        g_fwd_stats.dropped_invalid++;
        return -1;
    }

    /* Check TTL */
    if (!fwd_check_ttl(iph)) {
        /* TODO: Send ICMP TTL Exceeded */
        return -1;
    }

    /* Recalculate checksum */
    iph->check = 0;
    iph->check = ip_checksum(iph, iph->ihl * 4);

    /* Lookup destination in FIB */
    ocr_prefix_t dst = {
        .family        = OCR_AF_INET,
        .addr.v4.s_addr = iph->daddr,
        .prefixlen      = 32,
    };

    ocr_nexthop_t nexthop;
    int nh_count = ocr_fib_lookup(g_fib, &dst, &nexthop);

    if (nh_count <= 0) {
        g_fwd_stats.dropped_no_route++;
        return -1;  /* No route to host */
    }

    /* TODO: ARP resolution and actual packet transmission */
    /* For now, log forwarding decision */
    char dst_str[INET_ADDRSTRLEN];
    char nh_str[INET_ADDRSTRLEN];
    inet_ntop(AF_INET, &iph->daddr, dst_str, sizeof(dst_str));
    inet_ntop(AF_INET, &nexthop.gate.v4, nh_str, sizeof(nh_str));

    fprintf(stdout, "[fwd] IPv4 dst=%s via %s dev %s\n",
            dst_str, nh_str, nexthop.ifname);

    g_fwd_stats.forwarded_pkts++;
    g_fwd_stats.forwarded_bytes += len;

    return nexthop.ifindex;
}

/* Forward IPv6 packet */
int ocr_fwd_ipv6(uint8_t *pkt, uint32_t len)
{
    if (!g_fib || len < sizeof(struct ip6_hdr)) {
        g_fwd_stats.dropped_invalid++;
        return -1;
    }

    struct ip6_hdr *ip6h = (struct ip6_hdr *)pkt;

    /* Check hop limit */
    if (ip6h->ip6_hlim <= 1) {
        g_fwd_stats.dropped_ttl++;
        return -1;
    }
    ip6h->ip6_hlim--;

    /* Lookup destination */
    ocr_prefix_t dst = {
        .family     = OCR_AF_INET6,
        .prefixlen  = 128,
    };
    memcpy(&dst.addr.v6, &ip6h->ip6_dst, 16);

    ocr_nexthop_t nexthop;
    int nh_count = ocr_fib_lookup(g_fib, &dst, &nexthop);

    if (nh_count <= 0) {
        g_fwd_stats.dropped_no_route++;
        return -1;
    }

    g_fwd_stats.forwarded_pkts++;
    g_fwd_stats.forwarded_bytes += len;
    return nexthop.ifindex;
}

/* Process incoming Ethernet frame */
int ocr_fwd_process(uint8_t *frame, uint32_t len, int in_ifindex)
{
    (void)in_ifindex;
    if (!frame || len < sizeof(struct ethhdr)) {
        g_fwd_stats.dropped_invalid++;
        return -1;
    }

    struct ethhdr *eth = (struct ethhdr *)frame;
    uint16_t ethertype = ntohs(eth->h_proto);

    uint8_t *payload = frame + sizeof(struct ethhdr);
    uint32_t payload_len = len - sizeof(struct ethhdr);

    switch (ethertype) {
    case ETH_P_IP:
        return ocr_fwd_ipv4(payload, payload_len);
    case ETH_P_IPV6:
        return ocr_fwd_ipv6(payload, payload_len);
    case ETH_P_ARP:
        /* TODO: ARP handling */
        return 0;
    default:
        return -1;
    }
}

/* Get forwarding statistics */
void ocr_fwd_get_stats(uint64_t *forwarded, uint64_t *dropped_no_route,
                        uint64_t *dropped_ttl)
{
    if (forwarded)        *forwarded        = g_fwd_stats.forwarded_pkts;
    if (dropped_no_route) *dropped_no_route = g_fwd_stats.dropped_no_route;
    if (dropped_ttl)      *dropped_ttl      = g_fwd_stats.dropped_ttl;
}

void ocr_fwd_reset_stats(void)
{
    memset(&g_fwd_stats, 0, sizeof(g_fwd_stats));
}
