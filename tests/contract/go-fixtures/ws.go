// Package main: WebSocket fixture capture.
//
// The sync hub (/ws) is exercised two ways:
//
//  1. Real client over the httptest server for success cases: connect a
//     nhooyr.io/websocket client, drive broadcasts via server.OnEvent, and
//     capture the JSONL frames the client receives. This captures the real
//     upgrade + pump + broadcast path.
//  2. Handler-level via httptest.NewRequest + Recorder for rejection cases
//     (auth and Origin), where the hub returns an HTTP error BEFORE the
//     upgrade. This captures the 401/403 status + body without needing a
//     non-loopback network interface.
//
// Frame fixtures are written as JSONL (one JSON object per line) to
// golden/ws/<case>.jsonl. Rejection cases (no WS frames) are written as JSON
// to golden/ws/<case>.json with the HTTP status and body.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
	"nhooyr.io/websocket"
)

// wsFrame is one line in a JSONL frame fixture.
type wsFrame struct {
	// Dir is "recv" (frame received by the client) or "send" (broadcast the
	// harness drove). "send" lines document what the harness did so the Rust
	// differential runner can replay the same sequence.
	Dir   string          `json:"dir"`
	Type  string          `json:"type,omitempty"` // websocket message type, e.g. "text"
	Event json.RawMessage `json:"event,omitempty"`
	// Note carries a human-readable annotation for non-frame lines (e.g.
	// keepalive documentation).
	Note string `json:"note,omitempty"`
}

// captureWS runs all WebSocket cases and writes golden/ws/<case>.jsonl or .json.
func captureWS(h *harness, goldenDir string) error {
	outDir := filepath.Join(goldenDir, "ws")
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return fmt.Errorf("mkdir %s: %w", outDir, err)
	}

	// Success cases: real client over the httptest server.
	if err := captureWSSuccess(h, outDir, "ws_auth_success", 1); err != nil {
		return err
	}
	if err := captureWSSuccess(h, outDir, "ws_event_broadcast", 3); err != nil {
		return err
	}

	// Keepalive: pings are protocol-level control frames, not data frames, so
	// they do not appear as JSONL messages. Document this and record that the
	// connection stays open across the ping interval.
	if err := writeJSONLFile(filepath.Join(outDir, "ws_keepalive.jsonl"), []wsFrame{
		{Dir: "note", Note: "Keepalive is enforced via nhooyr.io/websocket Ping control frames every 30s (pingInterval) with a 10s timeout (pingTimeout). Pings are protocol-level and do not appear as data messages on the WS stream. A failed ping closes the connection so the read pump exits instead of leaking on a half-open peer. The Rust port must implement an equivalent keepalive that closes dead peers within pingTimeout of a missed ping."},
	}); err != nil {
		return fmt.Errorf("write keepalive fixture: %w", err)
	}

	// ?after= reconnect contract: documented for the black-box Rust runner,
	// which seeds durable events via REST (pair/revoke) rather than OnEvent.
	if err := writeJSONLFile(filepath.Join(outDir, "ws_after_replay.jsonl"), []wsFrame{
		{Dir: "note", Note: "Reconnect contract: GET /ws?after=<cursor> replays durable events with id > cursor, then transitions to live Hub broadcast (EventBus LiveFanout). Slow-client buffer-full resync is covered in src/sync/tests.rs, not black-box."},
		{Dir: "send", Note: "seed DeviceRevocationPending + DeviceRevocationCancelled via pair/revoke/cancel REST"},
		{Dir: "send", Note: "connect ws://host/ws?after=<id_of_pending>"},
		{Dir: "recv", Type: "text", Note: "replay DeviceRevocationCancelled (id > after)"},
		{Dir: "send", Note: "pair+revoke another device while WS connected"},
		{Dir: "recv", Type: "text", Note: "live DeviceRevocationPending via hub broadcast"},
	}); err != nil {
		return fmt.Errorf("write after_replay fixture: %w", err)
	}

	// Rejection cases: handler-level (no upgrade).
	if err := captureWSRejection(h, outDir, "ws_auth_rejection", false, ""); err != nil {
		return err
	}
	if err := captureWSRejection(h, outDir, "ws_origin_rejection", true, "http://evil.example.com"); err != nil {
		return err
	}
	return nil
}

// captureWSSuccess connects a real WS client to the httptest server, drives
// `nBroadcasts` synthetic broadcasts via server.OnEvent, and captures the
// received frames as JSONL. The connection is closed cleanly after capture.
func captureWSSuccess(h *harness, outDir, name string, nBroadcasts int) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	wsURL := strings.Replace(h.httpSrv.URL, "http://", "ws://", 1) + "/ws"
	// The server's CSRF check requires Origin == r.Host. nhooyr's client dial
	// does not set Origin by default, so set it explicitly to the server's
	// host (the loopback host:port the httptest server listens on). This
	// mirrors what a same-origin browser WS handshake would send.
	u, err := url.Parse(h.httpSrv.URL)
	if err != nil {
		return fmt.Errorf("parse http srv url: %w", err)
	}
	hdr := http.Header{}
	hdr.Set("Origin", "http://"+u.Host)
	conn, _, err := websocket.Dial(ctx, wsURL, &websocket.DialOptions{HTTPHeader: hdr})
	if err != nil {
		return fmt.Errorf("ws dial %s: %w", name, err)
	}
	defer func() { _ = conn.Close(websocket.StatusNormalClosure, "") }()
	conn.SetReadLimit(1 << 20)

	// Small goroutine that reads frames and sends them on a channel so the
	// main goroutine can interleave broadcasts and reads deterministically.
	type recv struct {
		typ websocket.MessageType
		data []byte
		err  error
	}
	recvCh := make(chan recv, nBroadcasts+4)
	go func() {
		for {
			typ, data, rerr := conn.Read(ctx)
			recvCh <- recv{typ: typ, data: data, err: rerr}
			if rerr != nil {
				return
			}
		}
	}()

	frames := make([]wsFrame, 0, nBroadcasts*2)
	for i := 0; i < nBroadcasts; i++ {
		// Drive a broadcast through the fully-wired server path
		// (recordEvent -> persist -> hub.Broadcast). Use a distinct event
		// type per broadcast so the fixture is self-describing.
		evt := interfaces.Event{
			Type:      interfaces.EventStreamUpdate,
			SessionID: "fixture-session",
			Role:      "agent",
			Content:   fmt.Sprintf("fixture broadcast %d", i),
		}
		h.server.OnEvent(evt)
		frames = append(frames, wsFrame{Dir: "send", Note: fmt.Sprintf("broadcast %d via server.OnEvent", i)})

		// Wait for the client to receive the frame.
		select {
		case r := <-recvCh:
			if r.err != nil {
				return fmt.Errorf("ws read %s frame %d: %v", name, i, r.err)
			}
			frames = append(frames, wsFrame{
				Dir:   "recv",
				Type:  msgTypeName(r.typ),
				Event: json.RawMessage(r.data),
			})
		case <-time.After(2 * time.Second):
			return fmt.Errorf("ws read %s frame %d: timeout", name, i)
		}
	}

	// Redact frame text (events may carry paths/secrets in other scenarios;
	// here they are synthetic but redaction keeps the policy uniform).
	for i := range frames {
		if len(frames[i].Event) > 0 {
			frames[i].Event = json.RawMessage(h.redactor.String(string(frames[i].Event)))
		}
	}
	return writeJSONLFile(filepath.Join(outDir, name+".jsonl"), frames)
}

// captureWSRejection exercises a pre-upgrade rejection at the handler level.
// When nonLoopback is true the request's RemoteAddr is set to a LAN address so
// the auth check runs (and fails without a credential => 401). When origin is
// non-empty, the Origin header is set to that value so the CSRF Origin check
// fails (=> 403). The result is written as JSON (status + body), not JSONL,
// since no WS frames are produced.
func captureWSRejection(h *harness, outDir, name string, nonLoopback bool, origin string) error {
	req := httptest.NewRequest(http.MethodGet, "/ws", nil)
	if nonLoopback {
		req.RemoteAddr = "10.0.0.7:1234"
	} else {
		req.RemoteAddr = "127.0.0.1:1234"
	}
	if origin != "" {
		req.Header.Set("Origin", origin)
	}
	rec := httptest.NewRecorder()
	h.server.Handler().ServeHTTP(rec, req)

	fix := restFixture{
		Method:      http.MethodGet,
		Path:        "/ws",
		Status:      rec.Code,
		ContentType: rec.Header().Get("Content-Type"),
		Body:        rec.Body.String(),
	}
	// Marshal + redact the full fixture so any non-deterministic value in the
	// body is scrubbed.
	data, err := json.MarshalIndent(fix, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal %s: %w", name, err)
	}
	redacted := h.redactor.String(string(data))
	return os.WriteFile(filepath.Join(outDir, name+".json"), []byte(redacted+"\n"), 0o644)
}

// msgTypeName maps a nhooyr MessageType to a stable string for the fixture.
func msgTypeName(t websocket.MessageType) string {
	switch t {
	case websocket.MessageText:
		return "text"
	case websocket.MessageBinary:
		return "binary"
	default:
		return fmt.Sprintf("type(%d)", int(t))
	}
}
