// server.go - OCR REST API Server
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package api

import (
	"context"
	"fmt"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/opencr/ocrd/internal/api/handlers"
	"github.com/opencr/ocrd/internal/api/middleware"
	"github.com/opencr/ocrd/internal/config"
	"github.com/opencr/ocrd/internal/netlink"
	"github.com/opencr/ocrd/internal/protocols/static"
	"github.com/opencr/ocrd/internal/services/dhcp"
	"github.com/opencr/ocrd/internal/services/dns"
	"github.com/opencr/ocrd/internal/services/vpn"
	"go.uber.org/zap"
)

// Services bundles all service dependencies for the API
type Services struct {
	Netlink *netlink.Manager
	Static  *static.Manager
	DHCP    *dhcp.Service
	DNS     *dns.Service
	VPN     *vpn.Service
	Config  *config.Config
}

// Server is the OCR REST API server
type Server struct {
	cfg      config.APIConfig
	services *Services
	logger   *zap.Logger
	engine   *gin.Engine
	httpSrv  *http.Server
}

// NewServer creates a new API server
func NewServer(cfg config.APIConfig, services *Services, logger *zap.Logger) *Server {
	if !services.Config.System.Debug {
		gin.SetMode(gin.ReleaseMode)
	}

	engine := gin.New()

	srv := &Server{
		cfg:      cfg,
		services: services,
		logger:   logger,
		engine:   engine,
	}

	srv.setupRoutes()
	return srv
}

func (s *Server) setupRoutes() {
	r := s.engine

	// Global middleware
	r.Use(middleware.Logger(s.logger))
	r.Use(middleware.CORS(s.cfg.CORSOrigin))
	r.Use(gin.Recovery())

	// Health check (no auth)
	r.GET("/health", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"status":  "ok",
			"version": "1.0.0",
			"time":    time.Now().UTC(),
		})
	})

	// Auth endpoint
	r.POST("/api/v1/auth/login", handlers.Login(s.cfg))

	// API v1 (requires auth)
	v1 := r.Group("/api/v1")
	v1.Use(middleware.JWT(s.cfg.JWTSecret))

	// System
	sys := handlers.NewSystemHandler(s.services.Config, s.logger)
	v1.GET("/system/info", sys.GetInfo)
	v1.GET("/system/stats", sys.GetStats)
	v1.PUT("/system/hostname", sys.SetHostname)
	v1.POST("/system/reboot", sys.Reboot)
	v1.POST("/system/shutdown", sys.Shutdown)

	// Interfaces
	iface := handlers.NewInterfaceHandler(s.services.Netlink, s.logger)
	v1.GET("/interfaces", iface.List)
	v1.GET("/interfaces/:name", iface.Get)
	v1.PUT("/interfaces/:name", iface.Update)
	v1.PUT("/interfaces/:name/up", iface.SetUp)
	v1.PUT("/interfaces/:name/down", iface.SetDown)
	v1.POST("/interfaces/:name/addresses", iface.AddAddress)
	v1.DELETE("/interfaces/:name/addresses/:cidr", iface.DelAddress)
	v1.POST("/interfaces/vlans", iface.CreateVLAN)

	// Routing
	routing := handlers.NewRoutingHandler(s.services.Netlink, s.services.Static, s.logger)
	v1.GET("/routing/table", routing.GetTable)
	v1.POST("/routing/static", routing.AddStatic)
	v1.DELETE("/routing/static", routing.DelStatic)
	v1.GET("/routing/protocols", routing.GetProtocols)

	// Firewall
	fw := handlers.NewFirewallHandler(s.logger)
	v1.GET("/firewall/rules", fw.List)
	v1.POST("/firewall/rules", fw.Add)
	v1.PUT("/firewall/rules/:id", fw.Update)
	v1.DELETE("/firewall/rules/:id", fw.Delete)
	v1.GET("/firewall/nat", fw.ListNAT)
	v1.POST("/firewall/nat", fw.AddNAT)
	v1.DELETE("/firewall/nat/:id", fw.DelNAT)
	v1.POST("/firewall/apply", fw.Apply)
	v1.POST("/firewall/flush", fw.Flush)

	// VPN
	vpnh := handlers.NewVPNHandler(s.services.VPN, s.logger)
	v1.GET("/vpn/tunnels", vpnh.List)
	v1.POST("/vpn/wireguard", vpnh.CreateWireGuard)
	v1.DELETE("/vpn/tunnels/:name", vpnh.Delete)
	v1.GET("/vpn/tunnels/:name/status", vpnh.Status)

	// DHCP
	dhcph := handlers.NewDHCPHandler(s.services.DHCP, s.logger)
	v1.GET("/dhcp/leases", dhcph.GetLeases)
	v1.GET("/dhcp/pools", dhcph.GetPools)
	v1.POST("/dhcp/pools", dhcph.AddPool)
	v1.DELETE("/dhcp/pools/:id", dhcph.DelPool)

	// DNS
	dnsh := handlers.NewDNSHandler(s.services.DNS, s.logger)
	v1.GET("/dns/config", dnsh.GetConfig)
	v1.PUT("/dns/config", dnsh.UpdateConfig)
	v1.GET("/dns/cache/stats", dnsh.CacheStats)
	v1.POST("/dns/cache/flush", dnsh.FlushCache)

	// QoS
	qos := handlers.NewQoSHandler(s.logger)
	v1.GET("/qos/policies", qos.List)
	v1.POST("/qos/policies", qos.Add)
	v1.DELETE("/qos/policies/:id", qos.Delete)
	v1.POST("/qos/apply", qos.Apply)

	// WebSocket for real-time stats
	r.GET("/ws/stats", middleware.JWT(s.cfg.JWTSecret), handlers.StatsWebSocket(s.services.Netlink))

	// Serve frontend static files
	r.Static("/ui", "/var/lib/ocr/webui")
	r.NoRoute(func(c *gin.Context) {
		c.File("/var/lib/ocr/webui/index.html")
	})

	s.logger.Info("routes configured",
		zap.Int("total_routes", len(r.Routes())),
	)
}

// Start starts the HTTP server
func (s *Server) Start() error {
	s.httpSrv = &http.Server{
		Addr:         s.cfg.ListenAddr,
		Handler:      s.engine,
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 30 * time.Second,
		IdleTimeout:  60 * time.Second,
	}

	if s.cfg.EnableTLS {
		return s.httpSrv.ListenAndServeTLS(s.cfg.TLSCert, s.cfg.TLSKey)
	}
	return s.httpSrv.ListenAndServe()
}

// Stop gracefully stops the HTTP server
func (s *Server) Stop(ctx context.Context) error {
	if s.httpSrv == nil {
		return nil
	}
	return s.httpSrv.Shutdown(ctx)
}

// Addr returns the server listen address
func (s *Server) Addr() string {
	return fmt.Sprintf("http://%s", s.cfg.ListenAddr)
}
