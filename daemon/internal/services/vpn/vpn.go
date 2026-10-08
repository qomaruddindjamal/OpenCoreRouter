// vpn.go - VPN Service Manager (WireGuard, OpenVPN, IPSec)
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package vpn

import (
	"context"
	"fmt"
	"os/exec"
	"sync"

	"github.com/opencr/ocrd/internal/config"
	"go.uber.org/zap"
)

// TunnelType identifies VPN tunnel type
type TunnelType string

const (
	TypeWireGuard TunnelType = "wireguard"
	TypeOpenVPN   TunnelType = "openvpn"
	TypeIPSec     TunnelType = "ipsec"
)

// Tunnel represents a VPN tunnel
type Tunnel struct {
	Name      string     `json:"name"`
	Type      TunnelType `json:"type"`
	Address   string     `json:"address"`
	Port      int        `json:"port"`
	Connected bool       `json:"connected"`
}

// Service manages VPN tunnels
type Service struct {
	cfg     config.VPNConfig
	logger  *zap.Logger
	mu      sync.RWMutex
	tunnels map[string]*Tunnel
}

// NewService creates a new VPN service
func NewService(cfg config.VPNConfig, logger *zap.Logger) *Service {
	return &Service{
		cfg:     cfg,
		logger:  logger,
		tunnels: make(map[string]*Tunnel),
	}
}

// Start initializes configured VPN tunnels
func (s *Service) Start(ctx context.Context) error {
	if !s.cfg.Enabled {
		return nil
	}

	// Init WireGuard interfaces
	for _, wgiface := range s.cfg.WireGuard.Interfaces {
		if err := s.CreateWireGuard(wgiface.Name, wgiface.Address, wgiface.ListenPort); err != nil {
			s.logger.Warn("failed to create WireGuard interface",
				zap.String("name", wgiface.Name),
				zap.Error(err),
			)
		}
	}

	return nil
}

// Stop cleans up VPN tunnels
func (s *Service) Stop() {
	s.mu.RLock()
	names := make([]string, 0, len(s.tunnels))
	for name := range s.tunnels {
		names = append(names, name)
	}
	s.mu.RUnlock()

	for _, name := range names {
		if err := s.deleteTunnel(name); err != nil {
			s.logger.Warn("failed to delete tunnel on shutdown",
				zap.String("name", name),
				zap.Error(err),
			)
		}
	}
}

// CreateWireGuard creates a WireGuard interface
func (s *Service) CreateWireGuard(name, address string, port int) error {
	// Check wg is available
	if _, err := exec.LookPath("wg"); err != nil {
		return fmt.Errorf("wg tool not found: %w", err)
	}

	// ip link add <name> type wireguard
	if err := exec.Command("ip", "link", "add", name, "type", "wireguard").Run(); err != nil {
		return fmt.Errorf("create WireGuard interface: %w", err)
	}

	// ip addr add <address> dev <name>
	if address != "" {
		if err := exec.Command("ip", "addr", "add", address, "dev", name).Run(); err != nil {
			exec.Command("ip", "link", "del", name).Run() //nolint:errcheck
			return fmt.Errorf("add address to WireGuard: %w", err)
		}
	}

	// wg set <name> listen-port <port>
	if port > 0 {
		if err := exec.Command("wg", "set", name,
			"listen-port", fmt.Sprintf("%d", port),
		).Run(); err != nil {
			s.logger.Warn("failed to set WireGuard listen-port", zap.Error(err))
		}
	}

	// ip link set <name> up
	if err := exec.Command("ip", "link", "set", name, "up").Run(); err != nil {
		return fmt.Errorf("bring WireGuard up: %w", err)
	}

	s.mu.Lock()
	s.tunnels[name] = &Tunnel{
		Name:      name,
		Type:      TypeWireGuard,
		Address:   address,
		Port:      port,
		Connected: true,
	}
	s.mu.Unlock()

	s.logger.Info("WireGuard interface created",
		zap.String("name", name),
		zap.String("address", address),
		zap.Int("port", port),
	)
	return nil
}

// AddWireGuardPeer adds a peer to a WireGuard interface
func (s *Service) AddWireGuardPeer(ifname, pubkey, endpoint string,
	allowedIPs []string, keepalive int) error {
	args := []string{"set", ifname, "peer", pubkey}
	if endpoint != "" {
		args = append(args, "endpoint", endpoint)
	}
	if len(allowedIPs) > 0 {
		for _, ip := range allowedIPs {
			args = append(args, "allowed-ips", ip)
		}
	}
	if keepalive > 0 {
		args = append(args, "persistent-keepalive", fmt.Sprintf("%d", keepalive))
	}

	if err := exec.Command("wg", args...).Run(); err != nil {
		return fmt.Errorf("add WireGuard peer: %w", err)
	}

	s.logger.Info("WireGuard peer added",
		zap.String("interface", ifname),
		zap.String("pubkey", pubkey[:8]+"..."),
	)
	return nil
}

func (s *Service) deleteTunnel(name string) error {
	s.mu.Lock()
	tunnel, ok := s.tunnels[name]
	if !ok {
		s.mu.Unlock()
		return fmt.Errorf("tunnel %s not found", name)
	}
	delete(s.tunnels, name)
	s.mu.Unlock()

	switch tunnel.Type {
	case TypeWireGuard:
		return exec.Command("ip", "link", "del", name).Run()
	}
	return nil
}

// List returns all tunnels
func (s *Service) List() []*Tunnel {
	s.mu.RLock()
	defer s.mu.RUnlock()
	result := make([]*Tunnel, 0, len(s.tunnels))
	for _, t := range s.tunnels {
		result = append(result, t)
	}
	return result
}
