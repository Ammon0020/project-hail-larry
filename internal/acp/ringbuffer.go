package acp

import "sync"

// ringBuffer is a thread-safe, fixed-capacity byte buffer that retains only the
// most recently written bytes. It implements io.Writer so it can be used as a
// process's Stderr, capturing a bounded tail of output.
type ringBuffer struct {
	mu   sync.Mutex
	buf  []byte
	size int
}

// newRingBuffer creates a ring buffer that retains up to size bytes.
func newRingBuffer(size int) *ringBuffer {
	if size <= 0 {
		size = 4096
	}
	return &ringBuffer{size: size}
}

// Write appends p, discarding the oldest bytes when the capacity is exceeded.
func (r *ringBuffer) Write(p []byte) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.buf = append(r.buf, p...)
	if len(r.buf) > r.size {
		r.buf = r.buf[len(r.buf)-r.size:]
	}
	return len(p), nil
}

// String returns the currently retained bytes as a string.
func (r *ringBuffer) String() string {
	r.mu.Lock()
	defer r.mu.Unlock()
	return string(r.buf)
}
