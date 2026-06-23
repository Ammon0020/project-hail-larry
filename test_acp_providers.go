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

func (d *dummyImpl) SessionUpdate(ctx context.Context, params acp.SessionNotification) error { return nil }
func (d *dummyImpl) RequestPermission(ctx context.Context, params acp.RequestPermissionRequest) (acp.RequestPermissionResponse, error) { return acp.RequestPermissionResponse{}, nil }
func (d *dummyImpl) ReadTextFile(ctx context.Context, params acp.ReadTextFileRequest) (acp.ReadTextFileResponse, error) { return acp.ReadTextFileResponse{}, nil }
func (d *dummyImpl) WriteTextFile(ctx context.Context, params acp.WriteTextFileRequest) (acp.WriteTextFileResponse, error) { return acp.WriteTextFileResponse{}, nil }
func (d *dummyImpl) CreateTerminal(ctx context.Context, params acp.CreateTerminalRequest) (acp.CreateTerminalResponse, error) { return acp.CreateTerminalResponse{}, nil }
func (d *dummyImpl) KillTerminal(ctx context.Context, params acp.KillTerminalRequest) (acp.KillTerminalResponse, error) { return acp.KillTerminalResponse{}, nil }
func (d *dummyImpl) TerminalOutput(ctx context.Context, params acp.TerminalOutputRequest) (acp.TerminalOutputResponse, error) { return acp.TerminalOutputResponse{}, nil }
func (d *dummyImpl) ReleaseTerminal(ctx context.Context, params acp.ReleaseTerminalRequest) (acp.ReleaseTerminalResponse, error) { return acp.ReleaseTerminalResponse{}, nil }
func (d *dummyImpl) WaitForTerminalExit(ctx context.Context, params acp.WaitForTerminalExitRequest) (acp.WaitForTerminalExitResponse, error) { return acp.WaitForTerminalExitResponse{}, nil }

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Usage: test_acp <command> [args...]")
		os.Exit(1)
	}
	cmdName := os.Args[1]
	args := os.Args[2:]

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, cmdName, args...)
	cwd, _ := os.Getwd()
	cmd.Dir = cwd // Set workdir
	stdin, _ := cmd.StdinPipe()
	stdout, _ := cmd.StdoutPipe()
	if err := cmd.Start(); err != nil {
		fmt.Printf("Failed to start %s: %v\n", cmdName, err)
		os.Exit(1)
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
		cmd.Process.Kill()
		os.Exit(1)
	}

	listReq := acp.UnstableListProvidersRequest{}
	listRes, err := client.UnstableListProviders(ctx, listReq)
	if err != nil {
		fmt.Printf("UnstableListProviders failed: %v\n", err)
	} else {
		b, _ := json.MarshalIndent(listRes, "", "  ")
		fmt.Printf("Providers for %s:\n%s\n", cmdName, string(b))
	}
	cmd.Process.Kill()
}
