// manager.go - Netlink Interface Manager
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package netlink

import (
	"context"
	"fmt"
	"net"
	"sync"

	"github.com/vishvananda/netlink"
	"go.uber.org/zap"
)

// Interface represents a network interface
type Interface struct {
	Name        string
	Index       int
	Flags       net.Flags
	HardwareAddr net.HardwareAddr
	MTU         int
	Addresses   []net.IPNet
	TxBytes     uint64
	RxBytes     uint64
	TxPackets   uint64
	RxPackets   uint64
	TxErrors    uint64
	RxErrors    uint64
	TxDropped   uint64
	RxDropped   uint64
}

// Route represents a routing table entry
type Route struct {
	Prefix   net.IPNet
	Gateway  net.IP
	LinkName string
	Protocol int
	Metric   int
	Table    int
}

// Manager manages network interfaces via netlink
type Manager struct {
	mu         sync.RWMutex
	interfaces map[string]*Interface
	routes     []Route
	logger     *zap.Logger

	// Callbacks
	OnIfaceUp   func(iface *Interface)
	OnIfaceDown func(iface *Interface)
	OnRouteAdd  func(route *Route)
	OnRouteDel  func(route *Route)
}

// NewManager creates a new netlink manager
func NewManager(logger *zap.Logger) (*Manager, error) {
	mgr := &Manager{
		interfaces: make(map[string]*Interface),
		logger:     logger,
	}
	return mgr, nil
}

// SyncInterfaces reads all interfaces from the kernel
func (m *Manager) SyncInterfaces() error {
	links, err := netlink.LinkList()
	if err != nil {
		return fmt.Errorf("LinkList: %w", err)
	}

	m.mu.Lock()
	defer m.mu.Unlock()

	for _, link := range links {
		attrs := link.Attrs()
		iface := &Interface{
			Name:         attrs.Name,
			Index:        attrs.Index,
			Flags:        net.Flags(attrs.Flags),
			HardwareAddr: attrs.HardwareAddr,
			MTU:          attrs.MTU,
		}

		// Get addresses
		addrs, err := netlink.AddrList(link, netlink.FAMILY_ALL)
		if err == nil {
			for _, addr := range addrs {
				if addr.IPNet != nil {
					iface.Addresses = append(iface.Addresses, *addr.IPNet)
				}
			}
		}

		m.interfaces[attrs.Name] = iface
		m.logger.Debug("synced interface",
			zap.String("name", attrs.Name),
			zap.Int("index", attrs.Index),
		)
	}

	return nil
}

// GetInterfaces returns all known interfaces
func (m *Manager) GetInterfaces() []*Interface {
	m.mu.RLock()
	defer m.mu.RUnlock()

	result := make([]*Interface, 0, len(m.interfaces))
	for _, iface := range m.interfaces {
		result = append(result, iface)
	}
	return result
}

// GetInterface returns a specific interface by name
func (m *Manager) GetInterface(name string) (*Interface, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	iface, ok := m.interfaces[name]
	return iface, ok
}

// SetInterfaceUp brings an interface up
func (m *Manager) SetInterfaceUp(name string) error {
	link, err := netlink.LinkByName(name)
	if err != nil {
		return fmt.Errorf("LinkByName(%s): %w", name, err)
	}
	if err := netlink.LinkSetUp(link); err != nil {
		return fmt.Errorf("LinkSetUp(%s): %w", name, err)
	}
	m.logger.Info("interface up", zap.String("name", name))
	return nil
}

// SetInterfaceDown brings an interface down
func (m *Manager) SetInterfaceDown(name string) error {
	link, err := netlink.LinkByName(name)
	if err != nil {
		return fmt.Errorf("LinkByName(%s): %w", name, err)
	}
	if err := netlink.LinkSetDown(link); err != nil {
		return fmt.Errorf("LinkSetDown(%s): %w", name, err)
	}
	m.logger.Info("interface down", zap.String("name", name))
	return nil
}

// SetMTU changes interface MTU
func (m *Manager) SetMTU(name string, mtu int) error {
	link, err := netlink.LinkByName(name)
	if err != nil {
		return fmt.Errorf("LinkByName(%s): %w", name, err)
	}
	if err := netlink.LinkSetMTU(link, mtu); err != nil {
		return fmt.Errorf("LinkSetMTU(%s, %d): %w", name, mtu, err)
	}
	return nil
}

// AddAddress adds an IP address to an interface
func (m *Manager) AddAddress(name, cidr string) error {
	link, err := netlink.LinkByName(name)
	if err != nil {
		return fmt.Errorf("LinkByName(%s): %w", name, err)
	}

	addr, err := netlink.ParseAddr(cidr)
	if err != nil {
		return fmt.Errorf("ParseAddr(%s): %w", cidr, err)
	}

	if err := netlink.AddrAdd(link, addr); err != nil {
		return fmt.Errorf("AddrAdd(%s, %s): %w", name, cidr, err)
	}

	m.logger.Info("added address",
		zap.String("interface", name),
		zap.String("address", cidr),
	)
	return nil
}

// DelAddress removes an IP address from an interface
func (m *Manager) DelAddress(name, cidr string) error {
	link, err := netlink.LinkByName(name)
	if err != nil {
		return fmt.Errorf("LinkByName(%s): %w", name, err)
	}

	addr, err := netlink.ParseAddr(cidr)
	if err != nil {
		return fmt.Errorf("ParseAddr(%s): %w", cidr, err)
	}

	if err := netlink.AddrDel(link, addr); err != nil {
		return fmt.Errorf("AddrDel(%s, %s): %w", name, cidr, err)
	}

	return nil
}

// AddRoute adds a route to the kernel routing table
func (m *Manager) AddRoute(prefix, gateway, ifname string, metric int) error {
	_, dst, err := net.ParseCIDR(prefix)
	if err != nil {
		return fmt.Errorf("ParseCIDR(%s): %w", prefix, err)
	}

	route := &netlink.Route{
		Dst:    dst,
		Metric: metric,
	}

	if gateway != "" {
		route.Gw = net.ParseIP(gateway)
	}

	if ifname != "" {
		link, err := netlink.LinkByName(ifname)
		if err != nil {
			return fmt.Errorf("LinkByName(%s): %w", ifname, err)
		}
		route.LinkIndex = link.Attrs().Index
	}

	if err := netlink.RouteAdd(route); err != nil {
		return fmt.Errorf("RouteAdd(%s via %s): %w", prefix, gateway, err)
	}

	m.logger.Info("added route",
		zap.String("prefix", prefix),
		zap.String("gateway", gateway),
		zap.String("interface", ifname),
	)
	return nil
}

// DelRoute removes a route from the kernel
func (m *Manager) DelRoute(prefix, gateway string) error {
	_, dst, err := net.ParseCIDR(prefix)
	if err != nil {
		return fmt.Errorf("ParseCIDR(%s): %w", prefix, err)
	}

	route := &netlink.Route{
		Dst: dst,
	}
	if gateway != "" {
		route.Gw = net.ParseIP(gateway)
	}

	if err := netlink.RouteDel(route); err != nil {
		return fmt.Errorf("RouteDel(%s): %w", prefix, err)
	}

	return nil
}

// GetRoutes returns all kernel routes
func (m *Manager) GetRoutes() ([]netlink.Route, error) {
	routes4, err := netlink.RouteList(nil, netlink.FAMILY_V4)
	if err != nil {
		return nil, fmt.Errorf("RouteList(IPv4): %w", err)
	}

	routes6, err := netlink.RouteList(nil, netlink.FAMILY_V6)
	if err != nil {
		return nil, fmt.Errorf("RouteList(IPv6): %w", err)
	}

	return append(routes4, routes6...), nil
}

// CreateVLAN creates a VLAN interface
func (m *Manager) CreateVLAN(parent string, vlanID int) (string, error) {
	parentLink, err := netlink.LinkByName(parent)
	if err != nil {
		return "", fmt.Errorf("parent interface %s not found: %w", parent, err)
	}

	vlanName := fmt.Sprintf("%s.%d", parent, vlanID)
	vlan := &netlink.Vlan{
		LinkAttrs: netlink.LinkAttrs{
			Name:        vlanName,
			ParentIndex: parentLink.Attrs().Index,
		},
		VlanId: vlanID,
	}

	if err := netlink.LinkAdd(vlan); err != nil {
		return "", fmt.Errorf("create VLAN %s: %w", vlanName, err)
	}

	if err := netlink.LinkSetUp(vlan); err != nil {
		netlink.LinkDel(vlan) //nolint:errcheck
		return "", fmt.Errorf("set VLAN %s up: %w", vlanName, err)
	}

	m.logger.Info("created VLAN",
		zap.String("interface", vlanName),
		zap.Int("vlan_id", vlanID),
	)
	return vlanName, nil
}

// Watch monitors netlink events for interface state changes
func (m *Manager) Watch(ctx context.Context) {
	updates := make(chan netlink.LinkUpdate)
	done := make(chan struct{})

	if err := netlink.LinkSubscribe(updates, done); err != nil {
		m.logger.Warn("failed to subscribe to netlink events", zap.Error(err))
		return
	}
	defer close(done)

	m.logger.Info("watching netlink events")

	for {
		select {
		case <-ctx.Done():
			return
		case update, ok := <-updates:
			if !ok {
				return
			}
			m.handleLinkUpdate(update)
		}
	}
}

func (m *Manager) handleLinkUpdate(update netlink.LinkUpdate) {
	attrs := update.Attrs()
	m.logger.Debug("link update",
		zap.String("name", attrs.Name),
		zap.Uint32("flags", uint32(attrs.Flags)),
	)

	m.mu.Lock()
	iface, exists := m.interfaces[attrs.Name]
	if !exists {
		iface = &Interface{
			Name:  attrs.Name,
			Index: attrs.Index,
		}
		m.interfaces[attrs.Name] = iface
	}
	iface.Flags = net.Flags(attrs.Flags)
	m.mu.Unlock()

	isUp := iface.Flags&net.FlagUp != 0
	if isUp && m.OnIfaceUp != nil {
		m.OnIfaceUp(iface)
	} else if !isUp && m.OnIfaceDown != nil {
		m.OnIfaceDown(iface)
	}
}
