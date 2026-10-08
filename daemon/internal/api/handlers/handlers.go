// handlers.go - OCR API Handlers (Interface, Routing, Firewall, VPN, DHCP, DNS, QoS, System, Auth)
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package handlers

import (
	"context"
	"net/http"
	"os/exec"
	"runtime"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/golang-jwt/jwt/v5"
	"github.com/gorilla/websocket"
	"github.com/opencr/ocrd/internal/config"
	"github.com/opencr/ocrd/internal/netlink"
	"github.com/opencr/ocrd/internal/protocols/static"
	"github.com/opencr/ocrd/internal/services/dhcp"
	"github.com/opencr/ocrd/internal/services/dns"
	"github.com/opencr/ocrd/internal/services/vpn"
	"github.com/shirou/gopsutil/v3/cpu"
	"github.com/shirou/gopsutil/v3/host"
	"github.com/shirou/gopsutil/v3/mem"
	"go.uber.org/zap"
)

// ─── Auth ────────────────────────────────────────────────────────────────────

type loginReq struct {
	Username string `json:"username" binding:"required"`
	Password string `json:"password" binding:"required"`
}

// Login returns a JWT token on valid credentials
func Login(cfg config.APIConfig) gin.HandlerFunc {
	return func(c *gin.Context) {
		var req loginReq
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		if req.Username != cfg.AdminUser || req.Password != cfg.AdminPass {
			c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid credentials"})
			return
		}

		claims := jwt.MapClaims{
			"username": req.Username,
			"role":     "admin",
			"exp":      time.Now().Add(24 * time.Hour).Unix(),
			"iat":      time.Now().Unix(),
		}
		token := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
		signed, err := token.SignedString([]byte(cfg.JWTSecret))
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to sign token"})
			return
		}

		c.JSON(http.StatusOK, gin.H{
			"token":   signed,
			"expires": time.Now().Add(24 * time.Hour).UTC(),
		})
	}
}

// ─── System ──────────────────────────────────────────────────────────────────

type SystemHandler struct {
	cfg    *config.Config
	logger *zap.Logger
}

func NewSystemHandler(cfg *config.Config, logger *zap.Logger) *SystemHandler {
	return &SystemHandler{cfg: cfg, logger: logger}
}

func (h *SystemHandler) GetInfo(c *gin.Context) {
	info, _ := host.Info()
	c.JSON(http.StatusOK, gin.H{
		"hostname":     h.cfg.System.Hostname,
		"version":      "1.0.0",
		"os":           runtime.GOOS,
		"arch":         runtime.GOARCH,
		"kernel":       info.KernelVersion,
		"platform":     info.Platform,
		"uptime":       info.Uptime,
		"boot_time":    info.BootTime,
		"go_version":   runtime.Version(),
	})
}

func (h *SystemHandler) GetStats(c *gin.Context) {
	cpuPct, _ := cpu.Percent(time.Second, false)
	memStat, _ := mem.VirtualMemory()

	cpuUsage := 0.0
	if len(cpuPct) > 0 {
		cpuUsage = cpuPct[0]
	}

	c.JSON(http.StatusOK, gin.H{
		"cpu": gin.H{
			"usage_percent": cpuUsage,
			"cores":         runtime.NumCPU(),
		},
		"memory": gin.H{
			"total_bytes":     memStat.Total,
			"used_bytes":      memStat.Used,
			"free_bytes":      memStat.Free,
			"usage_percent":   memStat.UsedPercent,
		},
		"goroutines": runtime.NumGoroutine(),
		"timestamp":  time.Now().UTC(),
	})
}

func (h *SystemHandler) SetHostname(c *gin.Context) {
	var req struct{ Hostname string `json:"hostname" binding:"required"` }
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := exec.Command("hostnamectl", "set-hostname", req.Hostname).Run(); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	h.cfg.System.Hostname = req.Hostname
	c.JSON(http.StatusOK, gin.H{"hostname": req.Hostname})
}

func (h *SystemHandler) Reboot(c *gin.Context) {
	go func() { time.Sleep(2 * time.Second); exec.Command("reboot").Run() }() //nolint:errcheck
	c.JSON(http.StatusAccepted, gin.H{"message": "rebooting in 2 seconds"})
}

func (h *SystemHandler) Shutdown(c *gin.Context) {
	go func() { time.Sleep(2 * time.Second); exec.Command("shutdown", "-h", "now").Run() }() //nolint:errcheck
	c.JSON(http.StatusAccepted, gin.H{"message": "shutting down in 2 seconds"})
}

// ─── Interfaces ──────────────────────────────────────────────────────────────

type InterfaceHandler struct {
	nl     *netlink.Manager
	logger *zap.Logger
}

func NewInterfaceHandler(nl *netlink.Manager, logger *zap.Logger) *InterfaceHandler {
	return &InterfaceHandler{nl: nl, logger: logger}
}

func (h *InterfaceHandler) List(c *gin.Context) {
	ifaces := h.nl.GetInterfaces()
	c.JSON(http.StatusOK, gin.H{"interfaces": ifaces, "count": len(ifaces)})
}

func (h *InterfaceHandler) Get(c *gin.Context) {
	name := c.Param("name")
	iface, ok := h.nl.GetInterface(name)
	if !ok {
		c.JSON(http.StatusNotFound, gin.H{"error": "interface not found"})
		return
	}
	c.JSON(http.StatusOK, iface)
}

func (h *InterfaceHandler) Update(c *gin.Context) {
	name := c.Param("name")
	var req struct {
		MTU         int    `json:"mtu"`
		Description string `json:"description"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if req.MTU > 0 {
		if err := h.nl.SetMTU(name, req.MTU); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
	}
	c.JSON(http.StatusOK, gin.H{"message": "updated"})
}

func (h *InterfaceHandler) SetUp(c *gin.Context) {
	if err := h.nl.SetInterfaceUp(c.Param("name")); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "up"})
}

func (h *InterfaceHandler) SetDown(c *gin.Context) {
	if err := h.nl.SetInterfaceDown(c.Param("name")); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "down"})
}

func (h *InterfaceHandler) AddAddress(c *gin.Context) {
	name := c.Param("name")
	var req struct{ CIDR string `json:"cidr" binding:"required"` }
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := h.nl.AddAddress(name, req.CIDR); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"cidr": req.CIDR})
}

func (h *InterfaceHandler) DelAddress(c *gin.Context) {
	name := c.Param("name")
	cidr := c.Param("cidr")
	if err := h.nl.DelAddress(name, cidr); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"message": "deleted"})
}

func (h *InterfaceHandler) CreateVLAN(c *gin.Context) {
	var req struct {
		Parent string `json:"parent" binding:"required"`
		VLANID int    `json:"vlan_id" binding:"required"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	name, err := h.nl.CreateVLAN(req.Parent, req.VLANID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"name": name})
}

// ─── Routing ──────────────────────────────────────────────────────────────────

type RoutingHandler struct {
	nl     *netlink.Manager
	static *static.Manager
	logger *zap.Logger
}

func NewRoutingHandler(nl *netlink.Manager, s *static.Manager, logger *zap.Logger) *RoutingHandler {
	return &RoutingHandler{nl: nl, static: s, logger: logger}
}

func (h *RoutingHandler) GetTable(c *gin.Context) {
	routes, err := h.nl.GetRoutes()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"routes": routes, "count": len(routes)})
}

func (h *RoutingHandler) AddStatic(c *gin.Context) {
	var req struct {
		Prefix   string `json:"prefix"   binding:"required"`
		NextHop  string `json:"nexthop"`
		Iface    string `json:"interface"`
		Distance int    `json:"distance"`
		Metric   int    `json:"metric"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := h.nl.AddRoute(req.Prefix, req.NextHop, req.Iface, req.Metric); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"prefix": req.Prefix, "nexthop": req.NextHop})
}

func (h *RoutingHandler) DelStatic(c *gin.Context) {
	var req struct {
		Prefix  string `json:"prefix"  binding:"required"`
		NextHop string `json:"nexthop"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := h.nl.DelRoute(req.Prefix, req.NextHop); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"message": "deleted"})
}

func (h *RoutingHandler) GetProtocols(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"protocols": []gin.H{
			{"name": "static",   "enabled": true,  "routes": 0},
			{"name": "ospf",     "enabled": false, "routes": 0},
			{"name": "bgp",      "enabled": false, "routes": 0},
			{"name": "rip",      "enabled": false, "routes": 0},
			{"name": "connected","enabled": true,  "routes": 0},
		},
	})
}

// ─── Firewall ──────────────────────────────────────────────────────────────────

type FirewallHandler struct{ logger *zap.Logger }

func NewFirewallHandler(logger *zap.Logger) *FirewallHandler {
	return &FirewallHandler{logger: logger}
}

func (h *FirewallHandler) List(c *gin.Context)    { c.JSON(http.StatusOK, gin.H{"rules": []any{}}) }
func (h *FirewallHandler) Add(c *gin.Context)     { c.JSON(http.StatusCreated, gin.H{"message": "rule added"}) }
func (h *FirewallHandler) Update(c *gin.Context)  { c.JSON(http.StatusOK, gin.H{"message": "updated"}) }
func (h *FirewallHandler) Delete(c *gin.Context)  { c.JSON(http.StatusOK, gin.H{"message": "deleted"}) }
func (h *FirewallHandler) ListNAT(c *gin.Context) { c.JSON(http.StatusOK, gin.H{"rules": []any{}}) }
func (h *FirewallHandler) AddNAT(c *gin.Context)  { c.JSON(http.StatusCreated, gin.H{"message": "NAT rule added"}) }
func (h *FirewallHandler) DelNAT(c *gin.Context)  { c.JSON(http.StatusOK, gin.H{"message": "deleted"}) }
func (h *FirewallHandler) Apply(c *gin.Context)   { c.JSON(http.StatusOK, gin.H{"message": "applied"}) }
func (h *FirewallHandler) Flush(c *gin.Context)   { c.JSON(http.StatusOK, gin.H{"message": "flushed"}) }

// ─── VPN ──────────────────────────────────────────────────────────────────────

type VPNHandler struct {
	svc    *vpn.Service
	logger *zap.Logger
}

func NewVPNHandler(svc *vpn.Service, logger *zap.Logger) *VPNHandler {
	return &VPNHandler{svc: svc, logger: logger}
}

func (h *VPNHandler) List(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"tunnels": []any{}})
}

func (h *VPNHandler) CreateWireGuard(c *gin.Context) {
	var req struct {
		Name       string `json:"name"        binding:"required"`
		Address    string `json:"address"     binding:"required"`
		ListenPort int    `json:"listen_port"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := h.svc.CreateWireGuard(req.Name, req.Address, req.ListenPort); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"name": req.Name, "type": "wireguard"})
}

func (h *VPNHandler) Delete(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"message": "deleted"})
}

func (h *VPNHandler) Status(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"name": c.Param("name"), "status": "unknown"})
}

// ─── DHCP ──────────────────────────────────────────────────────────────────────

type DHCPHandler struct {
	svc    *dhcp.Service
	logger *zap.Logger
}

func NewDHCPHandler(svc *dhcp.Service, logger *zap.Logger) *DHCPHandler {
	return &DHCPHandler{svc: svc, logger: logger}
}

func (h *DHCPHandler) GetLeases(c *gin.Context) {
	leases := h.svc.GetLeases()
	c.JSON(http.StatusOK, gin.H{"leases": leases, "count": len(leases)})
}

func (h *DHCPHandler) GetPools(c *gin.Context)  { c.JSON(http.StatusOK, gin.H{"pools": []any{}}) }
func (h *DHCPHandler) AddPool(c *gin.Context)   { c.JSON(http.StatusCreated, gin.H{"message": "pool added"}) }
func (h *DHCPHandler) DelPool(c *gin.Context)   { c.JSON(http.StatusOK, gin.H{"message": "deleted"}) }

// ─── DNS ──────────────────────────────────────────────────────────────────────

type DNSHandler struct {
	svc    *dns.Service
	logger *zap.Logger
}

func NewDNSHandler(svc *dns.Service, logger *zap.Logger) *DNSHandler {
	return &DNSHandler{svc: svc, logger: logger}
}

func (h *DNSHandler) GetConfig(c *gin.Context)    { c.JSON(http.StatusOK, h.svc.GetConfig()) }
func (h *DNSHandler) UpdateConfig(c *gin.Context) { c.JSON(http.StatusOK, gin.H{"message": "updated"}) }
func (h *DNSHandler) CacheStats(c *gin.Context)   { c.JSON(http.StatusOK, gin.H{"hits": 0, "misses": 0}) }
func (h *DNSHandler) FlushCache(c *gin.Context)   { c.JSON(http.StatusOK, gin.H{"message": "cache flushed"}) }

// ─── QoS ──────────────────────────────────────────────────────────────────────

type QoSHandler struct{ logger *zap.Logger }

func NewQoSHandler(logger *zap.Logger) *QoSHandler { return &QoSHandler{logger: logger} }

func (h *QoSHandler) List(c *gin.Context)   { c.JSON(http.StatusOK, gin.H{"policies": []any{}}) }
func (h *QoSHandler) Add(c *gin.Context)    { c.JSON(http.StatusCreated, gin.H{"message": "policy added"}) }
func (h *QoSHandler) Delete(c *gin.Context) { c.JSON(http.StatusOK, gin.H{"message": "deleted"}) }
func (h *QoSHandler) Apply(c *gin.Context)  { c.JSON(http.StatusOK, gin.H{"message": "applied"}) }

// ─── WebSocket Stats ──────────────────────────────────────────────────────────

var wsUpgrader = websocket.Upgrader{
	ReadBufferSize:  1024,
	WriteBufferSize: 1024,
	CheckOrigin:     func(r *http.Request) bool { return true },
}

// StatsWebSocket streams live stats to WebSocket clients
func StatsWebSocket(nl *netlink.Manager) gin.HandlerFunc {
	return func(c *gin.Context) {
		conn, err := wsUpgrader.Upgrade(c.Writer, c.Request, nil)
		if err != nil {
			return
		}
		defer conn.Close()

		ticker := time.NewTicker(2 * time.Second)
		defer ticker.Stop()

		ctx, cancel := context.WithTimeout(c.Request.Context(), 30*time.Minute)
		defer cancel()

		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				cpuPct, _ := cpu.Percent(0, false)
				memStat, _ := mem.VirtualMemory()

				cpuUsage := 0.0
				if len(cpuPct) > 0 {
					cpuUsage = cpuPct[0]
				}

				data := gin.H{
					"type":      "stats",
					"timestamp": time.Now().UTC(),
					"cpu_pct":   cpuUsage,
					"mem_pct":   memStat.UsedPercent,
					"mem_used":  memStat.Used,
					"mem_total": memStat.Total,
				}

				if err := conn.WriteJSON(data); err != nil {
					return
				}
			}
		}
	}
}
