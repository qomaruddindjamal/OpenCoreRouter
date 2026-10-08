// firewall4.go - OpenWrt firewall4/nftables Integration
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project
//
// OpenWrt 25.12.5 uses firewall4 as the firewall manager,
// which generates nftables rules from UCI /etc/config/firewall.
// OCR supports both UCI-based rules AND direct nftables for advanced use.

package firewall

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/opencr/ocrd/internal/uci"
	"go.uber.org/zap"
)

const (
	FW4Bin    = "/usr/sbin/fw4"
	NftBin    = "/usr/sbin/nft"
	NftOCRConf = "/etc/nftables.d/ocr.nft"

	// UCI firewall config path
	UCIFirewall = "/etc/config/firewall"

	// firewall4 inet table
	NFTTable  = "inet fw4"
	NFTChainInput   = "input"
	NFTChainForward = "forward"
	NFTChainOutput  = "output"
)

// RuleAction maps to nftables/UCI action verbs
type RuleAction string

const (
	ActionAccept RuleAction = "accept"
	ActionDrop   RuleAction = "drop"
	ActionReject RuleAction = "reject"
	ActionLog    RuleAction = "log"
)

// Direction maps to OpenWrt zone-based firewall
type Direction string

const (
	DirInput   Direction = "input"
	DirOutput  Direction = "output"
	DirForward Direction = "forward"
)

// UCIRule represents a firewall rule in UCI format
type UCIRule struct {
	Name        string     `json:"name"`
	Src         string     `json:"src"`          // source zone (e.g. "lan", "wan")
	Dst         string     `json:"dst"`          // dest zone
	SrcIP       string     `json:"src_ip"`
	DstIP       string     `json:"dest_ip"`
	SrcPort     string     `json:"src_port"`
	DstPort     string     `json:"dest_port"`
	Proto       string     `json:"proto"`        // tcp, udp, tcpudp, icmp, all
	Target      RuleAction `json:"target"`       // ACCEPT, DROP, REJECT
	Family      string     `json:"family"`       // ipv4, ipv6, any
	Enabled     bool       `json:"enabled"`
}

// NATRule represents a SNAT/DNAT/Masquerade rule
type NATRule struct {
	Name     string `json:"name"`
	Src      string `json:"src"`
	SrcIP    string `json:"src_ip"`
	DstIP    string `json:"dest_ip"`
	DstPort  string `json:"dest_port"`
	Target   string `json:"target"`   // SNAT, DNAT, MASQUERADE
	ToIP     string `json:"to_ip"`
	ToPort   string `json:"to_port"`
	OutIface string `json:"out_iface"`
}

// Manager manages OpenWrt firewall4 + direct nftables
type Manager struct {
	mu      sync.RWMutex
	uci     *uci.Client
	logger  *zap.Logger
	nextID  atomic.Int64
}

// NewManager creates a firewall manager for OpenWrt 25.12.5
func NewManager(uciClient *uci.Client, logger *zap.Logger) *Manager {
	return &Manager{uci: uciClient, logger: logger}
}

// ─── UCI-based firewall rules (firewall4 style) ──────────────────────────────

// AddUCIRule adds a rule via UCI → firewall4 reloads nftables
func (m *Manager) AddUCIRule(rule *UCIRule) (int, error) {
	if rule.Name == "" {
		rule.Name = fmt.Sprintf("ocr_rule_%d", m.nextID.Add(1))
	}

	id := int(m.nextID.Load())
	sectionName := fmt.Sprintf("ocr_rule_%d", id)

	b := m.uci.NewBatch("firewall")
	b.Set(fmt.Sprintf("firewall.%s", sectionName), "rule")
	b.Set(fmt.Sprintf("firewall.%s.name", sectionName), rule.Name)

	if rule.Src != "" {
		b.Set(fmt.Sprintf("firewall.%s.src", sectionName), rule.Src)
	}
	if rule.Dst != "" {
		b.Set(fmt.Sprintf("firewall.%s.dest", sectionName), rule.Dst)
	}
	if rule.SrcIP != "" {
		b.Set(fmt.Sprintf("firewall.%s.src_ip", sectionName), rule.SrcIP)
	}
	if rule.DstIP != "" {
		b.Set(fmt.Sprintf("firewall.%s.dest_ip", sectionName), rule.DstIP)
	}
	if rule.Proto != "" && rule.Proto != "all" {
		b.Set(fmt.Sprintf("firewall.%s.proto", sectionName), rule.Proto)
	}
	if rule.DstPort != "" {
		b.Set(fmt.Sprintf("firewall.%s.dest_port", sectionName), rule.DstPort)
	}
	if rule.SrcPort != "" {
		b.Set(fmt.Sprintf("firewall.%s.src_port", sectionName), rule.SrcPort)
	}
	b.Set(fmt.Sprintf("firewall.%s.target", sectionName), strings.ToUpper(string(rule.Target)))

	if rule.Family != "" {
		b.Set(fmt.Sprintf("firewall.%s.family", sectionName), rule.Family)
	}

	enabled := "1"
	if !rule.Enabled {
		enabled = "0"
	}
	b.Set(fmt.Sprintf("firewall.%s.enabled", sectionName), enabled)

	if err := b.Commit(); err != nil {
		return 0, fmt.Errorf("add UCI firewall rule: %w", err)
	}

	// Reload firewall4 to apply nftables rules
	if err := uci.ServiceReload("firewall"); err != nil {
		m.logger.Warn("firewall reload failed", zap.Error(err))
	}

	m.logger.Info("UCI firewall rule added",
		zap.String("name", rule.Name),
		zap.String("target", string(rule.Target)),
	)
	return id, nil
}

// DelUCIRule removes a firewall rule by UCI section name
func (m *Manager) DelUCIRule(sectionName string) error {
	if err := m.uci.Delete(fmt.Sprintf("firewall.%s", sectionName)); err != nil {
		return fmt.Errorf("delete UCI rule: %w", err)
	}
	if err := m.uci.Commit("firewall"); err != nil {
		return err
	}
	return uci.ServiceReload("firewall")
}

// ─── Direct nftables (advanced/dynamic rules) ────────────────────────────────

// NftAdd adds a raw nftables rule directly (immediate, no reload needed)
func (m *Manager) NftAdd(table, chain, rule string) error {
	cmd := exec.Command(NftBin, "add", "rule", table, chain, rule)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("nft add rule %s %s %q: %w (out=%s)", table, chain, rule, err, out)
	}
	m.logger.Debug("nft rule added",
		zap.String("table", table),
		zap.String("chain", chain),
		zap.String("rule", rule),
	)
	return nil
}

// NftFlush flushes the OCR custom chain
func (m *Manager) NftFlush() error {
	return exec.Command(NftBin, "flush", "ruleset").Run()
}

// ─── NAT via UCI (firewall4 redirect rules) ──────────────────────────────────

// AddMasquerade adds masquerade for a WAN interface
// In OpenWrt: firewall.@zone[1].masq=1 — but OCR adds it programmatically
func (m *Manager) AddMasquerade(outIface string) error {
	// Find WAN zone and enable masquerade
	b := m.uci.NewBatch("firewall")
	// OpenWrt convention: zone "wan" should have masq=1
	b.Set("firewall.@zone[1].masq", "1")
	b.Set("firewall.@zone[1].masq_src", "0.0.0.0/0")
	if err := b.Commit(); err != nil {
		return fmt.Errorf("set masquerade: %w", err)
	}
	return uci.ServiceReload("firewall")
}

// AddDNAT adds a port-forward (DNAT) rule via UCI
func (m *Manager) AddDNAT(name, proto, srcDstIP, dstPort, toIP, toPort string) error {
	sectionName := fmt.Sprintf("ocr_redirect_%d", m.nextID.Add(1))

	b := m.uci.NewBatch("firewall")
	b.Set(fmt.Sprintf("firewall.%s", sectionName), "redirect")
	b.Set(fmt.Sprintf("firewall.%s.name", sectionName), name)
	b.Set(fmt.Sprintf("firewall.%s.target", sectionName), "DNAT")
	b.Set(fmt.Sprintf("firewall.%s.src", sectionName), "wan")
	b.Set(fmt.Sprintf("firewall.%s.proto", sectionName), proto)
	b.Set(fmt.Sprintf("firewall.%s.src_dport", sectionName), dstPort)
	b.Set(fmt.Sprintf("firewall.%s.dest_ip", sectionName), toIP)
	b.Set(fmt.Sprintf("firewall.%s.dest_port", sectionName), toPort)
	if err := b.Commit(); err != nil {
		return err
	}
	return uci.ServiceReload("firewall")
}

// ─── OCR custom nftables file (persisted across firewall reloads) ─────────────

// WriteOCRNftFile writes a custom nftables file loaded by firewall4
// Place in /etc/nftables.d/ — firewall4 includes this dir automatically
func (m *Manager) WriteOCRNftFile(rules string) error {
	content := fmt.Sprintf(`# OCR Custom nftables rules
# Auto-generated by Open Core Router daemon
# OpenWrt 25.12.5 — firewall4 (nftables backend)
# DO NOT EDIT — managed by ocrd

table inet ocr {
  chain input {
    type filter hook input priority 0; policy accept;
%s
  }

  chain forward {
    type filter hook forward priority 0; policy accept;
  }

  chain output {
    type filter hook output priority 0; policy accept;
  }
}
`, rules)

	if err := os.WriteFile(NftOCRConf, []byte(content), 0644); err != nil {
		return fmt.Errorf("write nft conf: %w", err)
	}

	// Reload nftables immediately
	return exec.Command(NftBin, "-f", NftOCRConf).Run()
}

// GetFW4Status returns firewall4 status
func (m *Manager) GetFW4Status() (string, error) {
	out, err := exec.Command(FW4Bin, "status").Output()
	if err != nil {
		return "", err
	}
	return string(out), nil
}

// GetNftRules returns current nftables ruleset
func (m *Manager) GetNftRules() (string, error) {
	out, err := exec.Command(NftBin, "list", "ruleset").Output()
	if err != nil {
		return "", err
	}
	return string(out), nil
}
