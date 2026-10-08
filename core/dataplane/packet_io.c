/**
 * packet_io.c - Open Core Router Packet I/O Engine
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 * Copyright (C) 2026 Open Core Router Project
 *
 * Packet capture/injection using AF_PACKET sockets.
 * Future: XDP/eBPF for high-performance paths.
 */

#include "ocr_types.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>

#include <sys/socket.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <netpacket/packet.h>
#include <net/ethernet.h>
#include <net/if.h>
#include <arpa/inet.h>
#include <linux/if_packet.h>
#include <linux/filter.h>

#define OCR_PKT_RING_FRAMES   256
#define OCR_PKT_FRAME_SIZE    2048
#define OCR_PKT_MAX_PORTS     64
#define OCR_PKT_BATCH_SIZE    64

/* Packet buffer */
typedef struct ocr_pkt_buf {
    uint8_t  *data;
    uint32_t  len;
    int       ifindex;
    uint64_t  timestamp;
} ocr_pkt_buf_t;

/* Port (per-interface packet I/O) */
typedef struct ocr_pkt_port {
    char      name[OCR_IFNAME_SIZE];
    int       ifindex;
    int       sock_fd;
    uint8_t  *ring;
    uint32_t  ring_size;
    uint32_t  ring_pos;
    bool      active;

    /* Stats */
    uint64_t  rx_packets;
    uint64_t  tx_packets;
    uint64_t  rx_bytes;
    uint64_t  tx_bytes;
    uint64_t  rx_dropped;
} ocr_pkt_port_t;

/* Packet I/O engine */
typedef struct ocr_pkt_engine {
    ocr_pkt_port_t  ports[OCR_PKT_MAX_PORTS];
    int              port_count;
    pthread_t        rx_thread;
    bool             running;

    /* Rx callback */
    void (*on_packet)(ocr_pkt_buf_t *pkt, void *arg);
    void            *cb_arg;
} ocr_pkt_engine_t;

/* Global engine */
static ocr_pkt_engine_t g_engine;

/* ------------------------------------------------------------------ */

static int pkt_open_port(const char *ifname)
{
    int sock = socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL));
    if (sock < 0) {
        perror("socket(AF_PACKET)");
        return -1;
    }

    /* Set non-blocking */
    int flags = fcntl(sock, F_GETFL, 0);
    fcntl(sock, F_SETFL, flags | O_NONBLOCK);

    /* Bind to interface */
    struct ifreq ifr;
    memset(&ifr, 0, sizeof(ifr));
    strncpy(ifr.ifr_name, ifname, IFNAMSIZ - 1);

    if (ioctl(sock, SIOCGIFINDEX, &ifr) < 0) {
        perror("SIOCGIFINDEX");
        close(sock);
        return -1;
    }

    struct sockaddr_ll sll = {
        .sll_family   = AF_PACKET,
        .sll_protocol = htons(ETH_P_ALL),
        .sll_ifindex  = ifr.ifr_ifindex,
    };

    if (bind(sock, (struct sockaddr *)&sll, sizeof(sll)) < 0) {
        perror("bind(AF_PACKET)");
        close(sock);
        return -1;
    }

    /* Enable PACKET_MMAP for zero-copy ring buffer */
    int ver = TPACKET_V3;
    if (setsockopt(sock, SOL_PACKET, PACKET_VERSION, &ver, sizeof(ver)) < 0) {
        /* Fall back to normal mode */
        perror("PACKET_VERSION");
    }

    return sock;
}

int ocr_pkt_add_port(const char *ifname)
{
    if (g_engine.port_count >= OCR_PKT_MAX_PORTS) return -1;

    ocr_pkt_port_t *port = &g_engine.ports[g_engine.port_count];
    memset(port, 0, sizeof(*port));
    strncpy(port->name, ifname, OCR_IFNAME_SIZE - 1);

    port->sock_fd = pkt_open_port(ifname);
    if (port->sock_fd < 0) return -1;

    /* Get ifindex */
    struct ifreq ifr;
    memset(&ifr, 0, sizeof(ifr));
    strncpy(ifr.ifr_name, ifname, IFNAMSIZ - 1);
    if (ioctl(port->sock_fd, SIOCGIFINDEX, &ifr) == 0)
        port->ifindex = ifr.ifr_ifindex;

    port->active = true;
    g_engine.port_count++;

    fprintf(stdout, "[pkt_io] Added port: %s (ifindex=%d)\n",
            ifname, port->ifindex);
    return g_engine.port_count - 1;
}

int ocr_pkt_send(int port_idx, const uint8_t *data, uint32_t len)
{
    if (port_idx < 0 || port_idx >= g_engine.port_count) return -1;

    ocr_pkt_port_t *port = &g_engine.ports[port_idx];
    if (!port->active) return -1;

    struct sockaddr_ll sll = {
        .sll_family  = AF_PACKET,
        .sll_ifindex = port->ifindex,
    };

    ssize_t sent = sendto(port->sock_fd, data, len, 0,
                          (struct sockaddr *)&sll, sizeof(sll));
    if (sent < 0) return -1;

    port->tx_packets++;
    port->tx_bytes += len;
    return (int)sent;
}

static void *pkt_rx_thread(void *arg)
{
    (void)arg;
    uint8_t buf[OCR_PKT_FRAME_SIZE];

    while (g_engine.running) {
        for (int i = 0; i < g_engine.port_count; i++) {
            ocr_pkt_port_t *port = &g_engine.ports[i];
            if (!port->active) continue;

            ssize_t n = recv(port->sock_fd, buf, sizeof(buf), MSG_DONTWAIT);
            if (n <= 0) continue;

            port->rx_packets++;
            port->rx_bytes += n;

            if (g_engine.on_packet) {
                ocr_pkt_buf_t pkt = {
                    .data      = buf,
                    .len       = (uint32_t)n,
                    .ifindex   = port->ifindex,
                    .timestamp = 0,
                };
                g_engine.on_packet(&pkt, g_engine.cb_arg);
            }
        }
        /* Small sleep to avoid busy loop */
        usleep(100);
    }

    return NULL;
}

int ocr_pkt_engine_start(void (*on_packet)(ocr_pkt_buf_t *, void *),
                           void *arg)
{
    g_engine.on_packet = on_packet;
    g_engine.cb_arg    = arg;
    g_engine.running   = true;

    int ret = pthread_create(&g_engine.rx_thread, NULL, pkt_rx_thread, NULL);
    if (ret != 0) {
        g_engine.running = false;
        return -1;
    }
    return 0;
}

void ocr_pkt_engine_stop(void)
{
    g_engine.running = false;
    pthread_join(g_engine.rx_thread, NULL);

    for (int i = 0; i < g_engine.port_count; i++) {
        if (g_engine.ports[i].sock_fd >= 0)
            close(g_engine.ports[i].sock_fd);
    }
    g_engine.port_count = 0;
}
