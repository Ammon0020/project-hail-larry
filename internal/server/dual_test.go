package server

import (
	"context"
	"crypto/tls"
	"io"
	"net"
	"net/http"
	"strconv"
	"testing"
	"time"
)

// freePort returns a TCP port that is currently free on loopback. It does so
// by opening a listener and immediately closing it, so there is a small TOCTOU
// window — acceptable for tests that bind right after.
func freePort(t *testing.T) int {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen for free port: %v", err)
	}
	defer func() { _ = l.Close() }()
	return l.Addr().(*net.TCPAddr).Port
}

// httpsClient builds an http.Client that talks to the dual stack's HTTPS
// listener with InsecureSkipVerify (the self-signed test cert is not in any
// trust store). The daemon's CLI does the same for localhost.
func httpsClient(t *testing.T) *http.Client {
	t.Helper()
	return &http.Client{
		Timeout: 5 * time.Second,
		Transport: &http.Transport{
			TLSClientConfig: &tls.Config{InsecureSkipVerify: true}, // self-signed test cert; test-only.
		},
	}
}

// TestListenDualServesHealthOnBothSchemes verifies that ListenDual starts a
// cleartext HTTP listener and a TLS HTTPS listener simultaneously, and that
// GET /health returns 200 on both. This is the core "type http:// or https://
// without restarting" guarantee.
func TestListenDualServesHealthOnBothSchemes(t *testing.T) {
	srv := New(nil)

	httpPort := freePort(t)
	httpsPort := freePort(t)
	httpAddr := "127.0.0.1:" + itoa(httpPort)
	httpsAddr := "127.0.0.1:" + itoa(httpsPort)

	// Generate a self-signed cert for the HTTPS listener.
	certDir := t.TempDir()
	certPath, keyPath, err := EnsureSelfSignedCert(certDir, "127.0.0.1")
	if err != nil {
		t.Fatalf("ensure cert: %v", err)
	}

	errCh := srv.ListenDual(httpAddr, httpsAddr, certPath, keyPath)
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(ctx)
	})

	// Poll both listeners until they answer (bind + ListenAndServe is async).
	httpURL := "http://" + httpAddr + "/health"
	httpsURL := "https://" + httpsAddr + "/health"

	if !waitForOK(t, httpURL, http.DefaultClient, 3*time.Second) {
		t.Fatalf("HTTP listener never answered /health")
	}
	if !waitForOK(t, httpsURL, httpsClient(t), 3*time.Second) {
		t.Fatalf("HTTPS listener never answered /health")
	}

	// Both listeners must still be up (no early exit) — re-check after the
	// HTTPS check to catch a race where HTTP died after its first answer.
	if !waitForOK(t, httpURL, http.DefaultClient, time.Second) {
		t.Errorf("HTTP listener died after HTTPS check")
	}

	// No error should have been reported yet.
	select {
	case err := <-errCh:
		t.Fatalf("listener reported error during dual run: %v", err)
	default:
	}
}

// TestListenDualShutdownDrainsBoth verifies that Shutdown closes both the HTTP
// and HTTPS listeners and that ListenDual's goroutines exit cleanly
// (http.ErrServerClosed is suppressed, so the channel receives nil).
func TestListenDualShutdownDrainsBoth(t *testing.T) {
	srv := New(nil)

	httpAddr := "127.0.0.1:" + itoa(freePort(t))
	httpsAddr := "127.0.0.1:" + itoa(freePort(t))

	certDir := t.TempDir()
	certPath, keyPath, err := EnsureSelfSignedCert(certDir, "127.0.0.1")
	if err != nil {
		t.Fatalf("ensure cert: %v", err)
	}

	errCh := srv.ListenDual(httpAddr, httpsAddr, certPath, keyPath)

	// Wait for both listeners to be accepting connections before shutting
	// down, so we are testing the drain path rather than a not-yet-started
	// server.
	if !waitForOK(t, "http://"+httpAddr+"/health", http.DefaultClient, 3*time.Second) {
		t.Fatalf("HTTP listener never came up")
	}
	if !waitForOK(t, "https://"+httpsAddr+"/health", httpsClient(t), 3*time.Second) {
		t.Fatalf("HTTPS listener never came up")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := srv.Shutdown(ctx); err != nil {
		t.Fatalf("shutdown: %v", err)
	}

	// Both goroutines should report nil (ErrServerClosed suppressed) within
	// the timeout. Drain both buffered slots.
	for i := 0; i < 2; i++ {
		select {
		case err := <-errCh:
			if err != nil {
				t.Errorf("listener %d reported non-nil error after shutdown: %v", i, err)
			}
		case <-time.After(3 * time.Second):
			t.Fatalf("listener %d did not report after shutdown (hung)", i)
		}
	}

	// After shutdown, a new connection to either port must fail.
	if _, err := net.DialTimeout("tcp", httpAddr, 500*time.Millisecond); err == nil {
		t.Error("HTTP listener still accepting connections after shutdown")
	}
	if _, err := net.DialTimeout("tcp", httpsAddr, 500*time.Millisecond); err == nil {
		t.Error("HTTPS listener still accepting connections after shutdown")
	}
}

// TestListenDualHTTPSBindFailFailsFast verifies that when the HTTPS listener
// cannot bind (port already in use), ListenDual reports the error rather than
// silently leaving only HTTP running. The HTTP listener is torn down as part
// of the fail-fast so the daemon returns instead of hanging.
func TestListenDualHTTPSBindFailFailsFast(t *testing.T) {
	srv := New(nil)

	// Pre-occupy the HTTPS port so ListenDual's HTTPS bind fails.
	httpsPort := freePort(t)
	httpsAddr := "127.0.0.1:" + itoa(httpsPort)
	blocker, err := net.Listen("tcp", httpsAddr)
	if err != nil {
		t.Fatalf("pre-bind https port: %v", err)
	}
	defer func() { _ = blocker.Close() }()

	httpAddr := "127.0.0.1:" + itoa(freePort(t))

	certDir := t.TempDir()
	certPath, keyPath, err := EnsureSelfSignedCert(certDir, "127.0.0.1")
	if err != nil {
		t.Fatalf("ensure cert: %v", err)
	}

	errCh := srv.ListenDual(httpAddr, httpsAddr, certPath, keyPath)
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(ctx)
	})

	select {
	case err := <-errCh:
		if err == nil {
			t.Fatal("expected error from failed HTTPS bind, got nil")
		}
		// The error must mention the HTTPS listener / address.
		if !contains(err.Error(), "https") {
			t.Errorf("expected error to mention https listener, got: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("ListenDual did not report HTTPS bind failure within timeout")
	}
}

// TestListenDualHTTPBindFailFailsFast mirrors the HTTPS case for an HTTP bind
// failure: the HTTP listener cannot bind, and the error is reported.
func TestListenDualHTTPBindFailFailsFast(t *testing.T) {
	srv := New(nil)

	httpPort := freePort(t)
	httpAddr := "127.0.0.1:" + itoa(httpPort)
	blocker, err := net.Listen("tcp", httpAddr)
	if err != nil {
		t.Fatalf("pre-bind http port: %v", err)
	}
	defer func() { _ = blocker.Close() }()

	httpsAddr := "127.0.0.1:" + itoa(freePort(t))

	certDir := t.TempDir()
	certPath, keyPath, err := EnsureSelfSignedCert(certDir, "127.0.0.1")
	if err != nil {
		t.Fatalf("ensure cert: %v", err)
	}

	errCh := srv.ListenDual(httpAddr, httpsAddr, certPath, keyPath)
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(ctx)
	})

	select {
	case err := <-errCh:
		if err == nil {
			t.Fatal("expected error from failed HTTP bind, got nil")
		}
		if !contains(err.Error(), "http") {
			t.Errorf("expected error to mention http listener, got: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("ListenDual did not report HTTP bind failure within timeout")
	}
}

// TestShutdownNilWhenNeverStarted verifies the dual-mode Shutdown is still a
// no-op (returns nil) when neither listener was ever started, preserving the
// single-server contract used by tests that only exercise the handler.
func TestShutdownNilWhenNeverStarted(t *testing.T) {
	srv := New(nil)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := srv.Shutdown(ctx); err != nil {
		t.Errorf("expected nil shutdown error on unstarted server, got: %v", err)
	}
}

// itoa wraps strconv.Itoa so call sites in this file read concisely.
func itoa(i int) string {
	return strconv.Itoa(i)
}

// contains is a local strings.Contains alias so this test file does not need
// to import strings (other tests in the package do, but keeping this file
// self-contained makes it easier to grep for what it depends on).
func contains(s, sub string) bool {
	return len(sub) == 0 || (len(s) >= len(sub) && indexOf(s, sub) >= 0)
}

func indexOf(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}

// waitForOK polls url with client until it returns HTTP 200 or the timeout
// elapses. Returns whether a 200 was observed. Used to wait for the async
// ListenDual goroutines to bind and start serving.
func waitForOK(t *testing.T, url string, client *http.Client, timeout time.Duration) bool {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		resp, err := client.Get(url)
		if err == nil {
			_, _ = io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				return true
			}
		}
		time.Sleep(25 * time.Millisecond)
	}
	return false
}
