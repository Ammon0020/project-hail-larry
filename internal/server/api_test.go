package server

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/pairing"
	"golang.org/x/time/rate"
)

// resetPairLimiters clears the package-level pairing rate limiter map so tests
// start from a known state. The limiter map is package-level (see api.go) and
// would otherwise persist buckets across tests, making assertions on burst
// counts flaky.
func resetPairLimiters(t *testing.T) {
	t.Helper()
	pairLimitersMu.Lock()
	pairLimiters = make(map[string]*rate.Limiter)
	pairLimitersMu.Unlock()
}

// TestParseEventParamsLimitCap verifies that parseEventParams applies the
// 10000 upper cap on the ?limit query parameter (Finding 4.1). A client
// requesting ?limit=999999999 must be silently lowered to 10000 rather than
// allowed to fetch an unbounded result set.
func TestParseEventParamsLimitCap(t *testing.T) {
	cases := []struct {
		name  string
		limit string
		want  int
	}{
		{"empty uses default", "", 1000},
		{"zero uses default", "0", 1000},
		{"small explicit", "10", 10},
		{"exactly the cap", "10000", 10000},
		{"above the cap is lowered", "999999999", 10000},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			url := "/api/events"
			if tc.limit != "" {
				url += "?limit=" + tc.limit
			}
			req := httptest.NewRequest(http.MethodGet, url, nil)
			_, got := parseEventParams(req)
			if got != tc.want {
				t.Errorf("limit=%q: expected %d, got %d", tc.limit, tc.want, got)
			}
		})
	}
}

// TestPairInitiateRateLimited verifies that POST /api/pair/initiate is rate
// limited per IP (Finding 8.1). The first pairRateBurst requests from a single
// IP succeed (200); the next request is rejected with 429 Too Many Requests.
func TestPairInitiateRateLimited(t *testing.T) {
	resetPairLimiters(t)

	mgr := pairing.NewManager(t.TempDir())
	srv := New(&Deps{PairingMgr: mgr})

	// Use a distinct remote address so this test's limiter bucket is isolated
	// from any other test that touches the package-level map.
	const remoteAddr = "127.0.0.1:9999"

	for i := 0; i < pairRateBurst; i++ {
		req := httptest.NewRequest(http.MethodPost, "/api/pair/initiate", nil)
		req.RemoteAddr = remoteAddr
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, req)
		if rec.Code != http.StatusOK {
			t.Fatalf("request %d: expected 200, got %d (body: %s)", i+1, rec.Code, rec.Body.String())
		}
	}

	// The (burst+1)th request from the same IP must be rate limited.
	req := httptest.NewRequest(http.MethodPost, "/api/pair/initiate", nil)
	req.RemoteAddr = remoteAddr
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusTooManyRequests {
		t.Fatalf("expected 429 after burst, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestPairInitiateRateLimitPerIP verifies that the rate limit is per-IP: after
// exhausting one IP's bucket, a request from a different IP still succeeds.
func TestPairInitiateRateLimitPerIP(t *testing.T) {
	resetPairLimiters(t)

	mgr := pairing.NewManager(t.TempDir())
	srv := New(&Deps{PairingMgr: mgr})

	const ipA = "10.0.0.1:1234"
	const ipB = "10.0.0.2:1234"

	// Exhaust IP A's bucket.
	for i := 0; i < pairRateBurst; i++ {
		req := httptest.NewRequest(http.MethodPost, "/api/pair/initiate", nil)
		req.RemoteAddr = ipA
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, req)
		if rec.Code != http.StatusOK {
			t.Fatalf("ipA request %d: expected 200, got %d", i+1, rec.Code)
		}
	}

	// IP A is now blocked.
	req := httptest.NewRequest(http.MethodPost, "/api/pair/initiate", nil)
	req.RemoteAddr = ipA
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusTooManyRequests {
		t.Fatalf("ipA after burst: expected 429, got %d", rec.Code)
	}

	// IP B has its own bucket and should still succeed.
	req = httptest.NewRequest(http.MethodPost, "/api/pair/initiate", nil)
	req.RemoteAddr = ipB
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("ipB first request: expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestPairVerifyPasscodeRateLimited verifies that the verify-passcode endpoint
// is also covered by the per-IP rate limiter (Finding 8.1). A wrong passcode
// yields 401 but is still admitted (counts against the bucket); after the
// burst the same IP gets 429 regardless of body content.
func TestPairVerifyPasscodeRateLimited(t *testing.T) {
	resetPairLimiters(t)

	mgr := pairing.NewManager(t.TempDir())
	srv := New(&Deps{PairingMgr: mgr})

	const remoteAddr = "127.0.0.1:5555"
	body := `{"passcode":"wrong","deviceName":"dev"}`
	for i := 0; i < pairRateBurst; i++ {
		req := httptest.NewRequest(http.MethodPost, "/api/pair/verify-passcode", strings.NewReader(body))
		req.RemoteAddr = remoteAddr
		req.Header.Set("Content-Type", "application/json")
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, req)
		if rec.Code == http.StatusTooManyRequests {
			t.Fatalf("request %d: should not be rate limited yet, got 429", i+1)
		}
	}

	// Next request from the same IP must be 429 regardless of body.
	req := httptest.NewRequest(http.MethodPost, "/api/pair/verify-passcode", strings.NewReader(body))
	req.RemoteAddr = remoteAddr
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusTooManyRequests {
		t.Fatalf("expected 429 after burst, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// ----------------------------------------------------------------------------
// Grace-period pending action handler tests (Blueprint Sec 19, Sec 13)
// ----------------------------------------------------------------------------

// pairDeviceForTest creates a session, pairs a device, and returns the issued
// credential. It mirrors the pairing_test.go helper but lives here so the
// server tests can build a populated PairingMgr.
func pairDeviceForTest(t *testing.T, mgr *pairing.Manager) *pairing.DeviceCredential {
	t.Helper()
	session, err := mgr.CreateSession("localhost", 7337)
	if err != nil {
		t.Fatalf("create session: %v", err)
	}
	cred, err := mgr.VerifyPasscode(session.Passcode, "Device")
	if err != nil {
		t.Fatalf("verify passcode: %v", err)
	}
	return cred
}

// newPendingActionsServer builds a Server wired with a PairingMgr (with one
// paired device) and a Config carrying the given grace period and remote
// registration flag. The returned credential is the paired device.
func newPendingActionsServer(t *testing.T, graceSeconds int, allowRemote bool) (*Server, *pairing.DeviceCredential) {
	t.Helper()
	mgr := pairing.NewManager(t.TempDir())
	cred := pairDeviceForTest(t, mgr)
	srv := New(&Deps{
		PairingMgr: mgr,
		Config: &config.Config{
			RevocationGracePeriodSeconds:     graceSeconds,
			AllowRemoteWorkspaceRegistration: allowRemote,
		},
	})
	srv.RegisterPendingActionRoutes()
	return srv, cred
}

// loopbackReq is a helper that builds an httptest.Request with a loopback
// RemoteAddr so requireAuth's loopback bypass applies (the host browser hits
// the daemon via localhost and is trusted without a device credential).
func loopbackReq(method, target string, body io.Reader) *http.Request {
	req := httptest.NewRequest(method, target, body)
	req.RemoteAddr = "127.0.0.1:1234"
	return req
}

// decodePendingAction decodes a 202 Accepted body into a PendingActionInfo.
func decodePendingAction(t *testing.T, body []byte) pairing.PendingActionInfo {
	t.Helper()
	var info pairing.PendingActionInfo
	if err := json.Unmarshal(body, &info); err != nil {
		t.Fatalf("decode pending action: %v (body: %s)", err, body)
	}
	return info
}

// TestRevokeDeviceGracePeriod verifies that DELETE /api/devices/{id} returns
// 202 Accepted when a grace period is configured, and that the pending action
// appears in GET /api/pending-actions.
func TestRevokeDeviceGracePeriod(t *testing.T) {
	srv, cred := newPendingActionsServer(t, 300, false)

	req := loopbackReq(http.MethodDelete, "/api/devices/"+cred.ID, nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusAccepted {
		t.Fatalf("expected 202 Accepted, got %d (body: %s)", rec.Code, rec.Body.String())
	}

	info := decodePendingAction(t, rec.Body.Bytes())
	if info.DeviceID != cred.ID {
		t.Errorf("expected deviceId %s, got %s", cred.ID, info.DeviceID)
	}
	if info.Type != "revocation" {
		t.Errorf("expected type 'revocation', got %s", info.Type)
	}

	// The pending action must appear in the list endpoint.
	listReq := loopbackReq(http.MethodGet, "/api/pending-actions", nil)
	listRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(listRec, listReq)
	if listRec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", listRec.Code)
	}
	var pending []pairing.PendingActionInfo
	if err := json.Unmarshal(listRec.Body.Bytes(), &pending); err != nil {
		t.Fatalf("decode pending list: %v", err)
	}
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending action, got %d", len(pending))
	}
	if pending[0].ID != info.ID {
		t.Errorf("expected pending ID %s, got %s", info.ID, pending[0].ID)
	}
}

// TestRevokeDeviceImmediate verifies that DELETE /api/devices/{id} returns 200
// OK when the grace period is 0 (backward compatible).
func TestRevokeDeviceImmediate(t *testing.T) {
	srv, cred := newPendingActionsServer(t, 0, false)

	req := loopbackReq(http.MethodDelete, "/api/devices/"+cred.ID, nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestCancelRevocation verifies that POST /api/devices/cancel-revocation
// cancels a pending revocation and returns 200 OK.
func TestCancelRevocation(t *testing.T) {
	srv, cred := newPendingActionsServer(t, 300, false)

	// Start a pending revocation.
	delReq := loopbackReq(http.MethodDelete, "/api/devices/"+cred.ID, nil)
	delRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(delRec, delReq)
	if delRec.Code != http.StatusAccepted {
		t.Fatalf("expected 202, got %d", delRec.Code)
	}
	info := decodePendingAction(t, delRec.Body.Bytes())

	// Cancel it.
	cancelBody := `{"actionId":"` + info.ID + `"}`
	cancelReq := loopbackReq(http.MethodPost, "/api/devices/cancel-revocation", strings.NewReader(cancelBody))
	cancelReq.Header.Set("Content-Type", "application/json")
	cancelRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(cancelRec, cancelReq)
	if cancelRec.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d (body: %s)", cancelRec.Code, cancelRec.Body.String())
	}

	// The pending action must be gone.
	listReq := loopbackReq(http.MethodGet, "/api/pending-actions", nil)
	listRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(listRec, listReq)
	var pending []pairing.PendingActionInfo
	_ = json.Unmarshal(listRec.Body.Bytes(), &pending)
	if len(pending) != 0 {
		t.Errorf("expected 0 pending actions after cancel, got %d", len(pending))
	}
}

// TestCancelRevocationNotFound verifies that cancelling a non-existent action
// returns 404.
func TestCancelRevocationNotFound(t *testing.T) {
	srv, _ := newPendingActionsServer(t, 300, false)

	body := `{"actionId":"nonexistent"}`
	req := loopbackReq(http.MethodPost, "/api/devices/cancel-revocation", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestWorkspaceRegistrationDisabled verifies that POST /api/workspaces returns
// 403 when AllowRemoteWorkspaceRegistration is false (the default).
func TestWorkspaceRegistrationDisabled(t *testing.T) {
	srv, _ := newPendingActionsServer(t, 300, false)

	body := `{"path":"/some/path"}`
	req := loopbackReq(http.MethodPost, "/api/workspaces", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d (body: %s)", rec.Code, rec.Body.String())
	}
	if !strings.Contains(rec.Body.String(), "Remote workspace registration is disabled") {
		t.Errorf("expected disabled message in body, got: %s", rec.Body.String())
	}
}

// TestWorkspaceRegistrationEnabled verifies that POST /api/workspaces returns
// 202 Accepted with a pending action when remote registration is enabled and a
// grace period is configured.
func TestWorkspaceRegistrationEnabled(t *testing.T) {
	srv, _ := newPendingActionsServer(t, 300, true)

	body := `{"path":"/some/path"}`
	req := loopbackReq(http.MethodPost, "/api/workspaces", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusAccepted {
		t.Fatalf("expected 202 Accepted, got %d (body: %s)", rec.Code, rec.Body.String())
	}

	info := decodePendingAction(t, rec.Body.Bytes())
	if info.Type != "workspace_registration" {
		t.Errorf("expected type 'workspace_registration', got %s", info.Type)
	}
	if info.Path != "/some/path" {
		t.Errorf("expected path /some/path, got %s", info.Path)
	}
}

// TestCancelWorkspaceRegistration verifies that POST
// /api/workspaces/cancel-registration cancels a pending workspace registration.
func TestCancelWorkspaceRegistration(t *testing.T) {
	srv, _ := newPendingActionsServer(t, 300, true)

	// Start a pending registration.
	regBody := `{"path":"/some/path"}`
	regReq := loopbackReq(http.MethodPost, "/api/workspaces", strings.NewReader(regBody))
	regReq.Header.Set("Content-Type", "application/json")
	regRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(regRec, regReq)
	if regRec.Code != http.StatusAccepted {
		t.Fatalf("expected 202, got %d (body: %s)", regRec.Code, regRec.Body.String())
	}
	info := decodePendingAction(t, regRec.Body.Bytes())

	// Cancel it.
	cancelBody := `{"actionId":"` + info.ID + `"}`
	cancelReq := loopbackReq(http.MethodPost, "/api/workspaces/cancel-registration", strings.NewReader(cancelBody))
	cancelReq.Header.Set("Content-Type", "application/json")
	cancelRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(cancelRec, cancelReq)
	if cancelRec.Code != http.StatusOK {
		t.Fatalf("expected 200 OK, got %d (body: %s)", cancelRec.Code, cancelRec.Body.String())
	}

	// The pending action must be gone.
	listReq := loopbackReq(http.MethodGet, "/api/pending-actions", nil)
	listRec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(listRec, listReq)
	var pending []pairing.PendingActionInfo
	_ = json.Unmarshal(listRec.Body.Bytes(), &pending)
	if len(pending) != 0 {
		t.Errorf("expected 0 pending actions after cancel, got %d", len(pending))
	}
}

// TestRevokeDeviceGracePeriodExpires verifies that after the grace period
// elapses the device is revoked. Uses a short grace period with a real sleep.
func TestRevokeDeviceGracePeriodExpires(t *testing.T) {
	srv, cred := newPendingActionsServer(t, 0, false)
	// Override the config to a very short grace period for the test.
	srv.deps.Config.RevocationGracePeriodSeconds = 0
	// Use the pairing manager directly with a short grace period to test the
	// timer path end-to-end through the API would require a config change
	// mid-test; instead exercise the manager directly here since the API
	// path is covered by TestRevokeDeviceGracePeriod.
	const grace = 50 * time.Millisecond
	if _, err := srv.deps.PairingMgr.RequestRevocation(cred.ID, "test", grace); err != nil {
		t.Fatalf("request revocation: %v", err)
	}
	time.Sleep(150 * time.Millisecond)
	if srv.deps.PairingMgr.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected device to be revoked after grace period expired")
	}
}
