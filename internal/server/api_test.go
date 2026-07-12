package server

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

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
