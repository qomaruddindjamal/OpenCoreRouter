/**
 * ocr_types.h - Open Core Router Core Type Definitions
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 */

#ifndef OCR_TYPES_H
#define OCR_TYPES_H

#include <stdint.h>
#include <stdbool.h>
#include <sys/types.h>
#include <netinet/in.h>
#include <linux/if.h>

/* Version */
#define OCR_VERSION_MAJOR   1
#define OCR_VERSION_MINOR   0
#define OCR_VERSION_PATCH   0
#define OCR_VERSION_STRING  "1.0.0"

/* Limits */
#define OCR_MAX_INTERFACES      256
#define OCR_MAX_ROUTES          65536
#define OCR_MAX_VLANS           4094
#define OCR_MAX_VPN_TUNNELS     128
#define OCR_MAX_FIREWALL_RULES  8192
#define OCR_MAX_QOS_POLICIES    64
#define OCR_IFNAME_SIZE         IFNAMSIZ
#define OCR_MAX_NEXTHOPS        16

/* Address Family */
typedef enum {
    OCR_AF_UNSPEC = 0,
    OCR_AF_INET   = AF_INET,
    OCR_AF_INET6  = AF_INET6,
} ocr_af_t;

/* Protocol */
typedef enum {
    OCR_PROTO_UNSPEC  = 0,
    OCR_PROTO_STATIC  = 1,
    OCR_PROTO_KERNEL  = 2,
    OCR_PROTO_CONNECTED = 3,
    OCR_PROTO_OSPF    = 10,
    OCR_PROTO_BGP     = 11,
    OCR_PROTO_RIP     = 12,
    OCR_PROTO_ISIS    = 13,
} ocr_proto_t;

/* IP prefix */
typedef struct ocr_prefix {
    ocr_af_t         family;
    union {
        struct in_addr  v4;
        struct in6_addr v6;
    } addr;
    uint8_t          prefixlen;
} ocr_prefix_t;

/* Next hop */
typedef struct ocr_nexthop {
    ocr_af_t         family;
    union {
        struct in_addr  v4;
        struct in6_addr v6;
    } gate;
    int              ifindex;
    char             ifname[OCR_IFNAME_SIZE];
    uint32_t         weight;
    bool             active;
} ocr_nexthop_t;

/* Interface flags */
typedef enum {
    OCR_IFF_UP       = (1 << 0),
    OCR_IFF_RUNNING  = (1 << 1),
    OCR_IFF_LOOPBACK = (1 << 2),
    OCR_IFF_PROMISC  = (1 << 3),
    OCR_IFF_VLAN     = (1 << 4),
    OCR_IFF_BRIDGE   = (1 << 5),
    OCR_IFF_BOND     = (1 << 6),
    OCR_IFF_VPN      = (1 << 7),
} ocr_iff_t;

/* Interface */
typedef struct ocr_interface {
    char             name[OCR_IFNAME_SIZE];
    int              ifindex;
    uint32_t         flags;
    uint8_t          mac[6];
    uint32_t         mtu;
    uint32_t         speed;    /* Mbps */
    uint16_t         vlan_id;

    /* Addresses */
    ocr_prefix_t     *addr_list;
    int              addr_count;

    /* Statistics */
    uint64_t         rx_bytes;
    uint64_t         tx_bytes;
    uint64_t         rx_packets;
    uint64_t         tx_packets;
    uint64_t         rx_errors;
    uint64_t         tx_errors;
    uint64_t         rx_dropped;
    uint64_t         tx_dropped;
} ocr_interface_t;

/* Route entry */
typedef struct ocr_route {
    ocr_prefix_t     prefix;
    ocr_proto_t      protocol;
    uint8_t          type;       /* unicast, multicast, blackhole, etc. */
    uint8_t          distance;
    uint32_t         metric;
    uint32_t         table;      /* routing table ID */

    /* Next hops (ECMP support) */
    ocr_nexthop_t    nexthops[OCR_MAX_NEXTHOPS];
    int              nexthop_count;

    uint64_t         installed_time;
    bool             selected;
    bool             installed;  /* in FIB */
    bool             fib_used;
} ocr_route_t;

/* Firewall action */
typedef enum {
    OCR_FW_ACCEPT = 0,
    OCR_FW_DROP,
    OCR_FW_REJECT,
    OCR_FW_LOG,
    OCR_FW_RETURN,
} ocr_fw_action_t;

/* Firewall direction */
typedef enum {
    OCR_FW_IN  = 0,
    OCR_FW_OUT,
    OCR_FW_FORWARD,
} ocr_fw_direction_t;

/* Firewall rule */
typedef struct ocr_fw_rule {
    uint32_t         id;
    char             name[64];
    ocr_fw_direction_t direction;
    char             in_iface[OCR_IFNAME_SIZE];
    char             out_iface[OCR_IFNAME_SIZE];
    ocr_prefix_t     src;
    ocr_prefix_t     dst;
    uint16_t         src_port_min;
    uint16_t         src_port_max;
    uint16_t         dst_port_min;
    uint16_t         dst_port_max;
    uint8_t          protocol;   /* IPPROTO_TCP, IPPROTO_UDP, etc. */
    uint8_t          state;      /* conntrack state flags */
    ocr_fw_action_t  action;
    bool             enabled;
    bool             log;

    /* Counters */
    uint64_t         packets;
    uint64_t         bytes;
} ocr_fw_rule_t;

/* NAT type */
typedef enum {
    OCR_NAT_SNAT = 0,
    OCR_NAT_DNAT,
    OCR_NAT_MASQUERADE,
} ocr_nat_type_t;

/* NAT rule */
typedef struct ocr_nat_rule {
    uint32_t         id;
    ocr_nat_type_t   type;
    ocr_prefix_t     src;
    ocr_prefix_t     dst;
    ocr_prefix_t     to_addr;
    uint16_t         to_port;
    char             out_iface[OCR_IFNAME_SIZE];
    bool             enabled;

    /* Counters */
    uint64_t         packets;
    uint64_t         bytes;
} ocr_nat_rule_t;

/* QoS policy */
typedef struct ocr_qos_policy {
    uint32_t         id;
    char             name[64];
    char             iface[OCR_IFNAME_SIZE];
    uint64_t         rate_bps;        /* bytes per second */
    uint64_t         ceil_bps;
    uint32_t         burst_bytes;
    uint32_t         priority;
} ocr_qos_policy_t;

/* VPN type */
typedef enum {
    OCR_VPN_WIREGUARD = 0,
    OCR_VPN_OPENVPN,
    OCR_VPN_IPSEC,
} ocr_vpn_type_t;

/* VPN tunnel */
typedef struct ocr_vpn_tunnel {
    uint32_t         id;
    char             name[64];
    ocr_vpn_type_t   type;
    ocr_prefix_t     local_addr;
    ocr_prefix_t     remote_addr;
    uint16_t         local_port;
    uint16_t         remote_port;
    bool             enabled;
    bool             connected;
} ocr_vpn_tunnel_t;

/* Error codes */
typedef enum {
    OCR_OK            = 0,
    OCR_ERR_GENERIC   = -1,
    OCR_ERR_NOMEM     = -2,
    OCR_ERR_INVAL     = -3,
    OCR_ERR_NOTFOUND  = -4,
    OCR_ERR_EXISTS    = -5,
    OCR_ERR_SYSCALL   = -6,
    OCR_ERR_NETLINK   = -7,
    OCR_ERR_TIMEOUT   = -8,
    OCR_ERR_PERM      = -9,
    OCR_ERR_FULL      = -10,
} ocr_err_t;

/* Helper macros */
#define OCR_ARRAY_SIZE(x)     (sizeof(x) / sizeof((x)[0]))
#define OCR_MIN(a, b)         ((a) < (b) ? (a) : (b))
#define OCR_MAX(a, b)         ((a) > (b) ? (a) : (b))
#define OCR_UNUSED(x)         ((void)(x))

/* Prefix length to netmask (IPv4) */
static inline uint32_t ocr_prefixlen_to_mask(uint8_t prefixlen) {
    if (prefixlen == 0) return 0;
    return htonl(~((1 << (32 - prefixlen)) - 1));
}

#endif /* OCR_TYPES_H */
