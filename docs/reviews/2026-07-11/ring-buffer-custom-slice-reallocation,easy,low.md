# Custom Slice-Based "Ring Buffer" Reallocations

- **Difficulty:** easy
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/internal/acp/ringbuffer.go`
- **Lines:** 23-31

## Description

The `StderrBuffer` in `internal/acp` is implemented using a slice that continually appends incoming data and then slices from the end (`r.buf = r.buf[len(r.buf)-r.size:]`) to maintain a maximum size.
This is not a true circular ring buffer. Appending to a slice in this manner triggers frequent memory allocation, resizing, and memory copying. During high-frequency streaming writes from subagent processes, this implementation produces unnecessary garbage collector overhead and memory pressure.

## Recommendation

Replace the custom slice reallocation pattern with a proper zero-allocation circular buffer using either:
1. A standard circular/ring buffer library like **`github.com/armon/circularbuffer`** or **`github.com/smallnest/ringbuffer`**.
2. A simple custom ring buffer using a fixed-size byte array/slice with read and write pointers that wrap around (modulo arithmetic), avoiding allocations during writes.

## Verification

Code inspection of [internal/acp/ringbuffer.go#L23-L31](file:///media/adam/extex/projects/project-hail-larry/internal/acp/ringbuffer.go#L23-L31) shows that the `Write` method appends bytes directly to the end of the slice and then reslices it when it exceeds capacity, leaving the old slice header and underlying array elements to be reallocated/copied.
