//go:build windows

package daemon

import (
	"fmt"
	"os/exec"
	"strings"
)

// processExists checks whether a process with the given PID is running.
// On Windows, Signal(0) is not supported, so we use tasklist instead.
func processExists(pid int) bool {
	cmd := exec.Command("tasklist", "/FI", fmt.Sprintf("PID eq %d", pid), "/NH", "/FO", "CSV") //nolint:gosec // tasklist is a fixed system command; pid is parsed from the daemon pid file.
	output, err := cmd.Output()
	if err != nil {
		return false
	}
	// tasklist prints "INFO: No tasks are running which match the specified criteria." when no match.
	return len(output) > 0 && !strings.Contains(string(output), "No tasks")
}
