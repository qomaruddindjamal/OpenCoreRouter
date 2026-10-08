// main.go - Open Core Router Daemon (ocrd)
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/opencr/ocrd/internal/api"
	"github.com/opencr/ocrd/internal/config"
	"github.com/opencr/ocrd/internal/netlink"
	"github.com/opencr/ocrd/internal/protocols/static"
	"github.com/opencr/ocrd/internal/services/dhcp"
	"github.com/opencr/ocrd/internal/services/dns"
	"github.com/opencr/ocrd/internal/services/vpn"
	"go.uber.org/zap"
)

const (
	AppName    = "ocrd"
	AppVersion = "1.0.0"
	BannerText = `
 ██████╗  ██████╗██████╗ 
██╔═══██╗██╔════╝██╔══██╗
██║   ██║██║     ██████╔╝
██║   ██║██║     ██╔══██╗
╚██████╔╝╚██████╗██║  ██║
 ╚═════╝  ╚═════╝╚═╝  ╚═╝
Open Core Router v%s — Management Daemon
`
)

func main() {
	fmt.Printf(BannerText, AppVersion)

	// Load config
	cfgPath := "/etc/ocr/ocr.conf"
	if len(os.Args) > 1 {
		cfgPath = os.Args[1]
	}

	cfg, err := config.Load(cfgPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to load config: %v\n", err)
		// Use default config
		cfg = config.Default()
	}

	// Initialize logger
	var logger *zap.Logger
	if cfg.System.Debug {
		logger, _ = zap.NewDevelopment()
	} else {
		logger, _ = zap.NewProduction()
	}
	defer logger.Sync() //nolint:errcheck
	sugar := logger.Sugar()

	sugar.Infow("Starting OCR daemon",
		"version", AppVersion,
		"config", cfgPath,
		"api_addr", cfg.API.ListenAddr,
	)

	// Initialize netlink manager
	nlMgr, err := netlink.NewManager(logger)
	if err != nil {
		sugar.Fatalw("Failed to initialize netlink manager", "error", err)
	}

	// Sync initial interface state
	if err := nlMgr.SyncInterfaces(); err != nil {
		sugar.Warnw("Failed to sync interfaces", "error", err)
	}

	// Initialize static routing
	staticMgr := static.NewManager(nlMgr, logger)

	// Initialize services
	dhcpSvc := dhcp.NewService(cfg.Services.DHCP, logger)
	dnsSvc := dns.NewService(cfg.Services.DNS, logger)
	vpnSvc := vpn.NewService(cfg.Services.VPN, logger)

	// Initialize API server
	apiServer := api.NewServer(cfg.API, &api.Services{
		Netlink: nlMgr,
		Static:  staticMgr,
		DHCP:    dhcpSvc,
		DNS:     dnsSvc,
		VPN:     vpnSvc,
		Config:  cfg,
	}, logger)

	// Start services
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	// Start netlink event watcher
	go nlMgr.Watch(ctx)

	// Start active services
	if cfg.Services.DHCP.Enabled {
		if err := dhcpSvc.Start(ctx); err != nil {
			sugar.Warnw("DHCP service failed to start", "error", err)
		}
	}
	if cfg.Services.DNS.Enabled {
		if err := dnsSvc.Start(ctx); err != nil {
			sugar.Warnw("DNS service failed to start", "error", err)
		}
	}
	if cfg.Services.VPN.Enabled {
		if err := vpnSvc.Start(ctx); err != nil {
			sugar.Warnw("VPN service failed to start", "error", err)
		}
	}

	// Start API server in goroutine
	errCh := make(chan error, 1)
	go func() {
		sugar.Infow("API server listening", "addr", cfg.API.ListenAddr)
		if err := apiServer.Start(); err != nil {
			errCh <- err
		}
	}()

	// Wait for shutdown signal
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM, syscall.SIGHUP)

	select {
	case sig := <-sigCh:
		sugar.Infow("Received signal, shutting down", "signal", sig)
	case err := <-errCh:
		sugar.Errorw("API server error", "error", err)
	}

	// Graceful shutdown
	sugar.Info("Graceful shutdown initiated...")
	cancel()

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()

	if err := apiServer.Stop(shutdownCtx); err != nil {
		sugar.Warnw("API server shutdown error", "error", err)
	}

	dhcpSvc.Stop()
	dnsSvc.Stop()
	vpnSvc.Stop()

	sugar.Info("OCR daemon stopped")
}
