package daemon

import (
	"fmt"
	"net"
	"strconv"
)

// probePort attempts to bind host:port briefly and releases it. Returns a
// non-nil error when the address is already in use (or otherwise not
// bindable), so callers can fail before constructing the full daemon.
func probePort(host string, port int) error {
	if port <= 0 {
		return fmt.Errorf("invalid port %d", port)
	}
	addr := net.JoinHostPort(host, strconv.Itoa(port))
	ln, err := net.Listen("tcp", addr)
	if err != nil {
		return err
	}
	_ = ln.Close()
	return nil
}
