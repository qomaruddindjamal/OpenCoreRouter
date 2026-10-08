// static.go - Static Route Manager
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project

package static

import (
	"fmt"
	"sync"

	"github.com/opencr/ocrd/internal/netlink"
	"go.uber.org/zap"
)

// Route is a static route entry
type Route struct {
	ID       int    `json:"id"`
	Prefix   string `json:"prefix"`
	NextHop  string `json:"nexthop"`
	Iface    string `json:"interface"`
	Distance int    `json:"distance"`
	Metric   int    `json:"metric"`
	Active   bool   `json:"active"`
}

// Manager manages static routes
type Manager struct {
	mu     sync.RWMutex
	routes []*Route
	nextID int
	nl     *netlink.Manager
	logger *zap.Logger
}

// NewManager creates a static route manager
func NewManager(nl *netlink.Manager, logger *zap.Logger) *Manager {
	return &Manager{
		nl:     nl,
		logger: logger,
		nextID: 1,
	}
}

// Add installs a static route
func (m *Manager) Add(prefix, nexthop, iface string, distance, metric int) (*Route, error) {
	if err := m.nl.AddRoute(prefix, nexthop, iface, metric); err != nil {
		return nil, fmt.Errorf("add route: %w", err)
	}

	m.mu.Lock()
	r := &Route{
		ID:       m.nextID,
		Prefix:   prefix,
		NextHop:  nexthop,
		Iface:    iface,
		Distance: distance,
		Metric:   metric,
		Active:   true,
	}
	m.routes = append(m.routes, r)
	m.nextID++
	m.mu.Unlock()

	m.logger.Info("static route added",
		zap.String("prefix", prefix),
		zap.String("nexthop", nexthop),
		zap.String("iface", iface),
	)
	return r, nil
}

// Del removes a static route
func (m *Manager) Del(id int) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	for i, r := range m.routes {
		if r.ID == id {
			if err := m.nl.DelRoute(r.Prefix, r.NextHop); err != nil {
				return fmt.Errorf("del route: %w", err)
			}
			m.routes = append(m.routes[:i], m.routes[i+1:]...)
			m.logger.Info("static route removed", zap.Int("id", id))
			return nil
		}
	}
	return fmt.Errorf("route %d not found", id)
}

// List returns all static routes
func (m *Manager) List() []*Route {
	m.mu.RLock()
	defer m.mu.RUnlock()
	result := make([]*Route, len(m.routes))
	copy(result, m.routes)
	return result
}
