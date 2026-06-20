//go:build !windows

package daemon

import (
	"os"
	"syscall"
)

// stopProcess sends a termination signal to the process on Unix-like systems.
func stopProcess(proc *os.Process) error {
	return proc.Signal(syscall.SIGTERM)
}
