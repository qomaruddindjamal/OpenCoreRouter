// config.go - OCR Daemon Configuration
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package config

import (
	"fmt"
	"os"

	"gopkg.in/yaml.v3"
)

// Config is the top-level configuration structure
type Config struct {
	System   SystemConfig   `yaml:"system"`
	API      APIConfig      `yaml:"api"`
	Network  NetworkConfig  `yaml:"network"`
	Routing  RoutingConfig  `yaml:"routing"`
	Firewall FirewallConfig `yaml:"firewall"`
	NAT      NATConfig      `yaml:"nat"`
	QoS      QoSConfig      `yaml:"qos"`
	Services ServicesConfig `yaml:"services"`
}

// SystemConfig holds system-level settings
type SystemConfig struct {
	Hostname string `yaml:"hostname"`
	Debug    bool   `yaml:"debug"`
	LogFile  string `yaml:"log_file"`
	LogLevel string `yaml:"log_level"`
	PIDFile  string `yaml:"pid_file"`
	DataDir  string `yaml:"data_dir"`
}

// APIConfig holds REST API configuration
type APIConfig struct {
	ListenAddr string   `yaml:"listen_addr"`
	TLSCert    string   `yaml:"tls_cert"`
	TLSKey     string   `yaml:"tls_key"`
	EnableTLS  bool     `yaml:"enable_tls"`
	JWTSecret  string   `yaml:"jwt_secret"`
	AdminUser  string   `yaml:"admin_user"`
	AdminPass  string   `yaml:"admin_pass"`
	AllowedIPs []string `yaml:"allowed_ips"`
	CORSOrigin string   `yaml:"cors_origin"`
}

// NetworkConfig holds interface configuration
type NetworkConfig struct {
	Interfaces []InterfaceConfig `yaml:"interfaces"`
}

// InterfaceConfig holds per-interface config
type InterfaceConfig struct {
	Name        string   `yaml:"name"`
	Description string   `yaml:"description"`
	Addresses   []string `yaml:"addresses"`
	MTU         int      `yaml:"mtu"`
	Enabled     bool     `yaml:"enabled"`
	VLANID      int      `yaml:"vlan_id"`
}

// RoutingConfig holds routing protocol config
type RoutingConfig struct {
	StaticRoutes []StaticRoute `yaml:"static_routes"`
	OSPF         OSPFConfig    `yaml:"ospf"`
	BGP          BGPConfig     `yaml:"bgp"`
	RIP          RIPConfig     `yaml:"rip"`
}

// StaticRoute is a single static route entry
type StaticRoute struct {
	Prefix   string `yaml:"prefix"`
	NextHop  string `yaml:"nexthop"`
	Distance int    `yaml:"distance"`
	Metric   int    `yaml:"metric"`
	Table    int    `yaml:"table"`
}

// OSPFConfig holds OSPF configuration
type OSPFConfig struct {
	Enabled    bool     `yaml:"enabled"`
	RouterID   string   `yaml:"router_id"`
	ProcessID  int      `yaml:"process_id"`
	Areas      []string `yaml:"areas"`
	Interfaces []string `yaml:"interfaces"`
}

// BGPConfig holds BGP configuration
type BGPConfig struct {
	Enabled     bool       `yaml:"enabled"`
	ASN         uint32     `yaml:"asn"`
	RouterID    string     `yaml:"router_id"`
	Neighbors   []BGPNeigh `yaml:"neighbors"`
	Networks    []string   `yaml:"networks"`
}

// BGPNeigh is a BGP neighbor
type BGPNeigh struct {
	Address  string `yaml:"address"`
	RemoteAS uint32 `yaml:"remote_as"`
	Password string `yaml:"password"`
}

// RIPConfig holds RIP configuration
type RIPConfig struct {
	Enabled    bool     `yaml:"enabled"`
	Version    int      `yaml:"version"`
	Interfaces []string `yaml:"interfaces"`
	Networks   []string `yaml:"networks"`
}

// FirewallConfig holds firewall configuration
type FirewallConfig struct {
	Enabled       bool          `yaml:"enabled"`
	DefaultPolicy string        `yaml:"default_policy"`
	Rules         []FirewallRule `yaml:"rules"`
}

// FirewallRule is a single firewall rule
type FirewallRule struct {
	ID          int    `yaml:"id"`
	Name        string `yaml:"name"`
	Direction   string `yaml:"direction"`
	InInterface string `yaml:"in_interface"`
	OutInterface string `yaml:"out_interface"`
	Source      string `yaml:"source"`
	Destination string `yaml:"destination"`
	Protocol    string `yaml:"protocol"`
	SrcPort     string `yaml:"src_port"`
	DstPort     string `yaml:"dst_port"`
	State       string `yaml:"state"`
	Action      string `yaml:"action"`
	Log         bool   `yaml:"log"`
}

// NATConfig holds NAT configuration
type NATConfig struct {
	Enabled   bool        `yaml:"enabled"`
	Rules     []NATRule   `yaml:"rules"`
	Masquerade []Masquerade `yaml:"masquerade"`
}

// NATRule is a DNAT/SNAT rule
type NATRule struct {
	ID          int    `yaml:"id"`
	Type        string `yaml:"type"`
	Source      string `yaml:"source"`
	Destination string `yaml:"destination"`
	ToAddress   string `yaml:"to_address"`
	ToPort      int    `yaml:"to_port"`
	OutInterface string `yaml:"out_interface"`
}

// Masquerade rule
type Masquerade struct {
	OutInterface string `yaml:"out_interface"`
	SourceNetwork string `yaml:"source_network"`
}

// QoSConfig holds QoS/traffic shaping config
type QoSConfig struct {
	Enabled  bool         `yaml:"enabled"`
	Policies []QoSPolicy  `yaml:"policies"`
}

// QoSPolicy is a single QoS policy
type QoSPolicy struct {
	ID        int    `yaml:"id"`
	Name      string `yaml:"name"`
	Interface string `yaml:"interface"`
	Rate      string `yaml:"rate"`
	Ceil      string `yaml:"ceil"`
	Burst     string `yaml:"burst"`
	Priority  int    `yaml:"priority"`
}

// ServicesConfig holds all service configs
type ServicesConfig struct {
	DHCP DHCPConfig `yaml:"dhcp"`
	DNS  DNSConfig  `yaml:"dns"`
	VPN  VPNConfig  `yaml:"vpn"`
}

// DHCPConfig holds DHCP server config
type DHCPConfig struct {
	Enabled   bool         `yaml:"enabled"`
	Subnets   []DHCPSubnet `yaml:"subnets"`
}

// DHCPSubnet is a DHCP pool definition
type DHCPSubnet struct {
	Network   string   `yaml:"network"`
	RangeStart string  `yaml:"range_start"`
	RangeEnd   string  `yaml:"range_end"`
	Gateway   string   `yaml:"gateway"`
	DNS       []string `yaml:"dns"`
	LeaseTime int      `yaml:"lease_time"`
}

// DNSConfig holds DNS resolver config
type DNSConfig struct {
	Enabled     bool     `yaml:"enabled"`
	ListenAddr  string   `yaml:"listen_addr"`
	Forwarders  []string `yaml:"forwarders"`
	LocalZones  []string `yaml:"local_zones"`
}

// VPNConfig holds VPN config
type VPNConfig struct {
	Enabled   bool              `yaml:"enabled"`
	WireGuard WireGuardConfig   `yaml:"wireguard"`
	OpenVPN   OpenVPNConfig     `yaml:"openvpn"`
	IPSec     IPSecConfig       `yaml:"ipsec"`
}

// WireGuardConfig holds WireGuard configuration
type WireGuardConfig struct {
	Enabled    bool             `yaml:"enabled"`
	Interfaces []WGInterface    `yaml:"interfaces"`
}

// WGInterface is a WireGuard interface config
type WGInterface struct {
	Name       string    `yaml:"name"`
	Address    string    `yaml:"address"`
	PrivateKey string    `yaml:"private_key"`
	ListenPort int       `yaml:"listen_port"`
	Peers      []WGPeer  `yaml:"peers"`
}

// WGPeer is a WireGuard peer
type WGPeer struct {
	PublicKey    string   `yaml:"public_key"`
	AllowedIPs   []string `yaml:"allowed_ips"`
	Endpoint     string   `yaml:"endpoint"`
	PresharedKey string   `yaml:"preshared_key"`
	KeepaliveInt int      `yaml:"keepalive"`
}

// OpenVPNConfig holds OpenVPN configuration
type OpenVPNConfig struct {
	Enabled bool   `yaml:"enabled"`
	Mode    string `yaml:"mode"`
	Port    int    `yaml:"port"`
	Proto   string `yaml:"proto"`
	Dev     string `yaml:"dev"`
	CA      string `yaml:"ca"`
	Cert    string `yaml:"cert"`
	Key     string `yaml:"key"`
}

// IPSecConfig holds IPSec configuration
type IPSecConfig struct {
	Enabled     bool          `yaml:"enabled"`
	Connections []IPSecConn   `yaml:"connections"`
}

// IPSecConn is an IPSec connection
type IPSecConn struct {
	Name      string `yaml:"name"`
	Local     string `yaml:"local"`
	Remote    string `yaml:"remote"`
	PSK       string `yaml:"psk"`
	IKEVersion int   `yaml:"ike_version"`
}

// Load reads configuration from a YAML file
func Load(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read config file: %w", err)
	}

	cfg := &Config{}
	if err := yaml.Unmarshal(data, cfg); err != nil {
		return nil, fmt.Errorf("parse config: %w", err)
	}

	// Apply defaults
	cfg.applyDefaults()

	if err := cfg.Validate(); err != nil {
		return nil, fmt.Errorf("validate config: %w", err)
	}

	return cfg, nil
}

// Default returns a default configuration
func Default() *Config {
	cfg := &Config{}
	cfg.applyDefaults()
	return cfg
}

func (c *Config) applyDefaults() {
	if c.System.Hostname == "" {
		hostname, _ := os.Hostname()
		c.System.Hostname = hostname
	}
	if c.System.LogLevel == "" {
		c.System.LogLevel = "info"
	}
	if c.System.DataDir == "" {
		c.System.DataDir = "/var/lib/ocr"
	}
	if c.System.PIDFile == "" {
		c.System.PIDFile = "/run/ocrd.pid"
	}
	if c.API.ListenAddr == "" {
		c.API.ListenAddr = ":8080"
	}
	if c.API.JWTSecret == "" {
		c.API.JWTSecret = "changeme-please-use-secure-secret"
	}
	if c.API.AdminUser == "" {
		c.API.AdminUser = "admin"
	}
	if c.API.AdminPass == "" {
		c.API.AdminPass = "admin"
	}
	if c.Services.DNS.ListenAddr == "" {
		c.Services.DNS.ListenAddr = "127.0.0.1:53"
	}
}

// Validate checks the configuration for errors
func (c *Config) Validate() error {
	if c.API.ListenAddr == "" {
		return fmt.Errorf("api.listen_addr is required")
	}
	if c.BGPEnabled() && c.Routing.BGP.ASN == 0 {
		return fmt.Errorf("bgp.asn is required when BGP is enabled")
	}
	return nil
}

// BGPEnabled returns true if BGP is enabled
func (c *Config) BGPEnabled() bool {
	return c.Routing.BGP.Enabled
}

// OSPFEnabled returns true if OSPF is enabled
func (c *Config) OSPFEnabled() bool {
	return c.Routing.OSPF.Enabled
}
