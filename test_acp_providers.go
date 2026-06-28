package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"time"

	"github.com/coder/acp-go-sdk"
)

type dummyImpl struct{}

func (d *dummyImpl) SessionUpdate(_ context.Context, _ acp.SessionNotification) error {
	return nil
}
func (d *dummyImpl) RequestPermission(_ context.Context, _ acp.RequestPermissionRequest) (acp.RequestPermissionResponse, error) {
	return acp.RequestPermissionResponse{}, nil
}
func (d *dummyImpl) ReadTextFile(_ context.Context, _ acp.ReadTextFileRequest) (acp.ReadTextFileResponse, error) {
	return acp.ReadTextFileResponse{}, nil
}
func (d *dummyImpl) WriteTextFile(_ context.Context, _ acp.WriteTextFileRequest) (acp.WriteTextFileResponse, error) {
	return acp.WriteTextFileResponse{}, nil
}
func (d *dummyImpl) CreateTerminal(_ context.Context, _ acp.CreateTerminalRequest) (acp.CreateTerminalResponse, error) {
	return acp.CreateTerminalResponse{}, nil
}
func (d *dummyImpl) KillTerminal(_ context.Context, _ acp.KillTerminalRequest) (acp.KillTerminalResponse, error) {
	return acp.KillTerminalResponse{}, nil
}
func (d *dummyImpl) TerminalOutput(_ context.Context, _ acp.TerminalOutputRequest) (acp.TerminalOutputResponse, error) {
	return acp.TerminalOutputResponse{}, nil
}
func (d *dummyImpl) ReleaseTerminal(_ context.Context, _ acp.ReleaseTerminalRequest) (acp.ReleaseTerminalResponse, error) {
	return acp.ReleaseTerminalResponse{}, nil
}
func (d *dummyImpl) WaitForTerminalExit(_ context.Context, _ acp.WaitForTerminalExitRequest) (acp.WaitForTerminalExitResponse, error) {
	return acp.WaitForTerminalExitResponse{}, nil
}

func main() {
	os.Exit(run())
}

// run executes the test harness logic and returns a process exit code.
// Defers (such as context cancellation) run before main calls os.Exit,
// avoiding the exitAfterDefer pitfall.
func run() int {
	if len(os.Args) < 2 {
		fmt.Println("Usage: test_acp <command> [args...]")
		return 1
	}
	cmdName := os.Args[1]
	args := os.Args[2:]

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Subprocess command and args come from user input intentionally; this is
	// a test harness for launching ACP agent binaries.
	cmd := exec.CommandContext(ctx, cmdName, args...) //nolint:gosec // test harness intentionally launches a user-specified subprocess
	cwd, _ := os.Getwd()
	cmd.Dir = cwd // Set workdir
	stdin, _ := cmd.StdinPipe()
	stdout, _ := cmd.StdoutPipe()
	if err := cmd.Start(); err != nil {
		fmt.Printf("Failed to start %s: %v\n", cmdName, err)
		return 1
	}

	client := acp.NewClientSideConnection(&dummyImpl{}, stdin, stdout)

	initReq := acp.InitializeRequest{
		ClientInfo: &acp.Implementation{
			Name:    "test",
			Version: "1.0",
		},
		ClientCapabilities: acp.ClientCapabilities{},
	}

	_, err := client.Initialize(ctx, initReq)
	if err != nil {
		fmt.Printf("Initialize failed: %v\n", err)
		if killErr := cmd.Process.Kill(); killErr != nil {
			fmt.Printf("Failed to kill process: %v\n", killErr)
		}
		return 1
	}

	listReq := acp.UnstableListProvidersRequest{}
	listRes, err := client.UnstableListProviders(ctx, listReq)
	if err != nil {
		fmt.Printf("UnstableListProviders failed: %v\n", err)
	} else {
		b, _ := json.MarshalIndent(listRes, "", "  ")
		fmt.Printf("Providers for %s:\n%s\n", cmdName, string(b))
	}
	if killErr := cmd.Process.Kill(); killErr != nil {
		fmt.Printf("Failed to kill process: %v\n", killErr)
	}
	return 0
}
