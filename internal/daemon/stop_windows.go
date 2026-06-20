//go:build windows

package daemon

import (
	"fmt"
	"os"
	"os/exec"
	"strconv"
)

// stopProcess terminates the process on Windows using taskkill.
// Windows doesn't support Unix signals, so we use taskkill /F /PID.
func stopProcess(proc *os.Process) error {
	// Try taskkill with the PID.
	cmd := exec.Command("taskkill", "/F", "/PID", strconv.Itoa(proc.Pid)) //nolint:gosec // taskkill is a fixed system command; proc.Pid comes from os.FindProcess.
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("taskkill: %w", err)
	}
	return nil
}
