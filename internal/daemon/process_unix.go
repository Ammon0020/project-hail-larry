//go:build !windows

package daemon

import (
	"os"
	"syscall"
)

// processExists checks whether a process with the given PID is running.
// On Unix, sending signal 0 checks existence without actually sending a signal.
func processExists(pid int) bool {
	proc, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return proc.Signal(syscall.Signal(0)) == nil
}
