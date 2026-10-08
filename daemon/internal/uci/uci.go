// uci.go - OpenWrt UCI (Unified Configuration Interface) Wrapper
//
// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 Open Core Router Project
//
// OpenWrt 25.12.5 uses UCI as the primary configuration system.
// All network, firewall, DHCP, VPN configs are stored in /etc/config/
// and manipulated via the 'uci' command or ubus RPC.

package uci

import (
	"bytes"
	"fmt"
	"os/exec"
	"strings"

	"go.uber.org/zap"
)

const (
	UCIBin  = "/sbin/uci"
	UBUSBin = "/bin/ubus"
)

// Client is a UCI configuration client
type Client struct {
	logger *zap.Logger
}

// NewClient creates a new UCI client
func NewClient(logger *zap.Logger) *Client {
	return &Client{logger: logger}
}

// Get reads a UCI value: uci get <config>.<section>.<option>
func (c *Client) Get(path string) (string, error) {
	out, err := exec.Command(UCIBin, "get", path).Output()
	if err != nil {
		return "", fmt.Errorf("uci get %s: %w", path, err)
	}
	return strings.TrimSpace(string(out)), nil
}

// Set writes a UCI value: uci set <config>.<section>.<option>=<value>
func (c *Client) Set(path, value string) error {
	cmd := exec.Command(UCIBin, "set", fmt.Sprintf("%s=%s", path, value))
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("uci set %s=%s: %w (out=%s)", path, value, err, out)
	}
	c.logger.Debug("uci set", zap.String("path", path), zap.String("value", value))
	return nil
}

// SetList sets a UCI list value
func (c *Client) SetList(path string, values []string) error {
	// First delete existing list
	exec.Command(UCIBin, "delete", path).Run() //nolint:errcheck

	for _, v := range values {
		cmd := exec.Command(UCIBin, "add_list", fmt.Sprintf("%s=%s", path, v))
		if out, err := cmd.CombinedOutput(); err != nil {
			return fmt.Errorf("uci add_list %s: %w (out=%s)", path, err, out)
		}
	}
	return nil
}

// Delete deletes a UCI section or option
func (c *Client) Delete(path string) error {
	cmd := exec.Command(UCIBin, "delete", path)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("uci delete %s: %w (out=%s)", path, err, out)
	}
	return nil
}

// Commit saves pending UCI changes to disk: uci commit <config>
func (c *Client) Commit(config string) error {
	cmd := exec.Command(UCIBin, "commit", config)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("uci commit %s: %w (out=%s)", config, err, out)
	}
	c.logger.Debug("uci commit", zap.String("config", config))
	return nil
}

// Revert discards pending UCI changes
func (c *Client) Revert(config string) error {
	return exec.Command(UCIBin, "revert", config).Run()
}

// AddSection adds a new UCI section: uci set config.name=type
func (c *Client) AddSection(config, name, sectionType string) error {
	return c.Set(fmt.Sprintf("%s.%s", config, name), sectionType)
}

// Show returns all UCI values for a config
func (c *Client) Show(config string) (map[string]string, error) {
	out, err := exec.Command(UCIBin, "show", config).Output()
	if err != nil {
		return nil, fmt.Errorf("uci show %s: %w", config, err)
	}

	result := make(map[string]string)
	for _, line := range strings.Split(string(out), "\n") {
		if idx := strings.Index(line, "="); idx > 0 {
			k := strings.TrimSpace(line[:idx])
			v := strings.Trim(strings.TrimSpace(line[idx+1:]), "'")
			result[k] = v
		}
	}
	return result, nil
}

// Batch performs multiple UCI operations atomically
type Batch struct {
	client *Client
	ops    []string
	config string
}

// NewBatch creates a batch UCI operation
func (c *Client) NewBatch(config string) *Batch {
	return &Batch{client: c, config: config}
}

// Set adds a set operation to the batch
func (b *Batch) Set(path, value string) *Batch {
	b.ops = append(b.ops, fmt.Sprintf("set %s=%s", path, value))
	return b
}

// Delete adds a delete operation to the batch
func (b *Batch) Delete(path string) *Batch {
	b.ops = append(b.ops, fmt.Sprintf("delete %s", path))
	return b
}

// Commit executes all batch operations
func (b *Batch) Commit() error {
	for _, op := range b.ops {
		parts := strings.Fields(op)
		cmd := exec.Command(UCIBin, parts...)
		if out, err := cmd.CombinedOutput(); err != nil {
			return fmt.Errorf("uci batch op '%s': %w (out=%s)", op, err, out)
		}
	}
	return b.client.Commit(b.config)
}

// ─── ubus RPC ────────────────────────────────────────────────────────────────

// UbusCall calls an ubus method and returns JSON output
// Example: ubus call network.interface.lan status
func UbusCall(object, method string, args map[string]interface{}) ([]byte, error) {
	argsJSON := "{}"
	if len(args) > 0 {
		var parts []string
		for k, v := range args {
			parts = append(parts, fmt.Sprintf("%q:%q", k, fmt.Sprint(v)))
		}
		argsJSON = "{" + strings.Join(parts, ",") + "}"
	}

	cmd := exec.Command(UBUSBin, "call", object, method, argsJSON)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("ubus call %s %s: %w (stderr=%s)", object, method, err, stderr.String())
	}

	return stdout.Bytes(), nil
}

// UbusList lists ubus objects matching a path
func UbusList(path string) ([]byte, error) {
	args := []string{"list"}
	if path != "" {
		args = append(args, path)
	}
	return exec.Command(UBUSBin, args...).Output()
}

// ─── Service control via procd/ubus ──────────────────────────────────────────

// ServiceReload reloads an OpenWrt init.d service
func ServiceReload(name string) error {
	return exec.Command("/etc/init.d/"+name, "reload").Run()
}

// ServiceRestart restarts an OpenWrt init.d service
func ServiceRestart(name string) error {
	return exec.Command("/etc/init.d/"+name, "restart").Run()
}

// ServiceStart starts an OpenWrt init.d service
func ServiceStart(name string) error {
	return exec.Command("/etc/init.d/"+name, "start").Run()
}

// ServiceStop stops an OpenWrt init.d service
func ServiceStop(name string) error {
	return exec.Command("/etc/init.d/"+name, "stop").Run()
}

// ─── Network helpers (netifd via UCI) ────────────────────────────────────────

// GetInterfaceStatus queries netifd for interface status via ubus
func GetInterfaceStatus(iface string) ([]byte, error) {
	return UbusCall(fmt.Sprintf("network.interface.%s", iface), "status", nil)
}

// SetInterfaceIP sets IP address via UCI network config
func SetInterfaceIP(c *Client, iface, ipaddr, netmask string) error {
	b := c.NewBatch("network")
	b.Set(fmt.Sprintf("network.%s", iface), "interface")
	b.Set(fmt.Sprintf("network.%s.proto", iface), "static")
	b.Set(fmt.Sprintf("network.%s.ipaddr", iface), ipaddr)
	b.Set(fmt.Sprintf("network.%s.netmask", iface), netmask)
	if err := b.Commit(); err != nil {
		return err
	}
	return ServiceReload("network")
}

// SetInterfaceDHCP configures interface as DHCP client
func SetInterfaceDHCP(c *Client, iface string) error {
	b := c.NewBatch("network")
	b.Set(fmt.Sprintf("network.%s", iface), "interface")
	b.Set(fmt.Sprintf("network.%s.proto", iface), "dhcp")
	if err := b.Commit(); err != nil {
		return err
	}
	return ServiceReload("network")
}

// SetWireGuardInterface configures a WireGuard interface via UCI
// OpenWrt 25.12 uses WireGuard as native kernel module (kmod-wireguard)
func SetWireGuardInterface(c *Client, iface, privateKey, listenPort string,
	addresses []string) error {
	b := c.NewBatch("network")
	b.Set(fmt.Sprintf("network.%s", iface), "interface")
	b.Set(fmt.Sprintf("network.%s.proto", iface), "wireguard")
	b.Set(fmt.Sprintf("network.%s.private_key", iface), privateKey)
	if listenPort != "" {
		b.Set(fmt.Sprintf("network.%s.listen_port", iface), listenPort)
	}
	if err := b.Commit(); err != nil {
		return err
	}
	if err := c.SetList(fmt.Sprintf("network.%s.addresses", iface), addresses); err != nil {
		return err
	}
	if err := c.Commit("network"); err != nil {
		return err
	}
	return ServiceReload("network")
}
