package cli

import (
	"bufio"
	"context"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"
)

// ExtractRequest is the request sent to Python extractor
type ExtractRequest struct {
	URL  string `json:"url"`
	HTML string `json:"html"`
}

// ExtractResponse is the response from Python extractor
type ExtractResponse struct {
	Text   string `json:"text"`
	Status string `json:"status"`
	Error  string `json:"error,omitempty"`
}

// Extractor manages the Python extraction process and socket communication
type Extractor struct {
	socketPath string
	cmd        *exec.Cmd
	mu         sync.Mutex
	connPool   chan net.Conn
	poolSize   int
	closed     bool
}

// NewExtractor creates and starts the Python extractor process
func NewExtractor(poolSize int) (*Extractor, error) {
	if poolSize < 1 {
		poolSize = 1
	}

	socketPath := filepath.Join(os.TempDir(), fmt.Sprintf("stock-news-extractor-%d-%d.sock", os.Getpid(), time.Now().UnixNano()))

	e := &Extractor{
		socketPath: socketPath,
		poolSize:   poolSize,
		connPool:   make(chan net.Conn, poolSize),
	}

	if err := e.start(); err != nil {
		return nil, err
	}

	return e, nil
}

// start launches the Python extractor process
func (e *Extractor) start() error {
	// Remove existing socket file if present
	os.Remove(e.socketPath)

	e.cmd = exec.Command("uv", "run", "python", "extractor.py", "--socket", e.socketPath)
	e.cmd.Stderr = os.Stderr

	// Create a new process group so we can kill all child processes
	e.cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}

	stdout, err := e.cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("failed to create stdout pipe: %w", err)
	}

	if err := e.cmd.Start(); err != nil {
		return fmt.Errorf("failed to start python extractor: %w", err)
	}

	// Wait for ready signal from Python
	scanner := bufio.NewScanner(stdout)
	ready := make(chan bool, 1)
	go func() {
		for scanner.Scan() {
			line := scanner.Text()
			if strings.HasPrefix(line, "READY:") {
				ready <- true
				return
			}
		}
		ready <- false
	}()

	select {
	case ok := <-ready:
		if !ok {
			e.killProcessGroup()
			return fmt.Errorf("python extractor failed to start")
		}
	case <-time.After(30 * time.Second):
		e.killProcessGroup()
		return fmt.Errorf("timeout waiting for python extractor to start")
	}

	// Initialize connection pool
	for i := 0; i < e.poolSize; i++ {
		conn, err := net.Dial("unix", e.socketPath)
		if err != nil {
			e.Close()
			return fmt.Errorf("failed to connect to extractor: %w", err)
		}
		e.connPool <- conn
	}

	return nil
}

// killProcessGroup kills the entire process group
func (e *Extractor) killProcessGroup() {
	if e.cmd != nil && e.cmd.Process != nil {
		// Kill the entire process group (negative PID)
		pgid, err := syscall.Getpgid(e.cmd.Process.Pid)
		if err == nil {
			syscall.Kill(-pgid, syscall.SIGTERM)
			time.Sleep(100 * time.Millisecond)
			syscall.Kill(-pgid, syscall.SIGKILL)
		}
		e.cmd.Process.Kill()
		e.cmd.Wait()
	}
}

// Extract sends HTML to Python and returns extracted text
func (e *Extractor) Extract(ctx context.Context, url, html string) (*ExtractResponse, error) {
	// Get connection from pool
	var conn net.Conn
	select {
	case conn = <-e.connPool:
	case <-ctx.Done():
		return nil, ctx.Err()
	}

	healthy := true
	// Return connection to pool when done
	defer func() {
		if conn == nil {
			return
		}
		if !e.closed && healthy {
			e.connPool <- conn
			return
		}
		_ = conn.Close()
		if e.closed {
			return
		}
		replacement, err := net.Dial("unix", e.socketPath)
		if err != nil {
			return
		}
		select {
		case e.connPool <- replacement:
		default:
			_ = replacement.Close()
		}
	}()

	if dl, ok := ctx.Deadline(); ok {
		_ = conn.SetDeadline(dl)
	} else {
		_ = conn.SetDeadline(time.Now().Add(60 * time.Second))
	}
	defer conn.SetDeadline(time.Time{})

	req := ExtractRequest{URL: url, HTML: html}
	if err := writeMessage(conn, req); err != nil {
		healthy = false
		return nil, fmt.Errorf("failed to send request: %w", err)
	}

	var resp ExtractResponse
	if err := readMessage(conn, &resp); err != nil {
		healthy = false
		return nil, fmt.Errorf("failed to read response: %w", err)
	}

	return &resp, nil
}

// Close shuts down the Python extractor
func (e *Extractor) Close() error {
	e.mu.Lock()
	if e.closed {
		e.mu.Unlock()
		return nil
	}
	e.closed = true
	e.mu.Unlock()

	// Drain and close all connections in pool
	done := make(chan struct{})
	go func() {
		for i := 0; i < e.poolSize; i++ {
			select {
			case conn := <-e.connPool:
				conn.Close()
			case <-time.After(time.Second):
				// Timeout waiting for connection
			}
		}
		close(done)
	}()

	select {
	case <-done:
	case <-time.After(5 * time.Second):
		// Timeout waiting for pool drain
	}

	// Kill the process group
	e.killProcessGroup()

	// Clean up socket file
	_ = os.Remove(e.socketPath)

	return nil
}

// writeMessage writes a length-prefixed JSON message
func writeMessage(conn net.Conn, msg interface{}) error {
	data, err := json.Marshal(msg)
	if err != nil {
		return err
	}

	// Write 4-byte length prefix (big-endian)
	header := make([]byte, 4)
	binary.BigEndian.PutUint32(header, uint32(len(data)))

	if err := writeAll(conn, header); err != nil {
		return err
	}
	if err := writeAll(conn, data); err != nil {
		return err
	}

	return nil
}

// readMessage reads a length-prefixed JSON message
func readMessage(conn net.Conn, v interface{}) error {
	// Read 4-byte length prefix
	header := make([]byte, 4)
	if _, err := io.ReadFull(conn, header); err != nil {
		return err
	}

	length := binary.BigEndian.Uint32(header)

	// Read JSON payload
	data := make([]byte, length)
	if _, err := io.ReadFull(conn, data); err != nil {
		return err
	}

	return json.Unmarshal(data, v)
}

func writeAll(conn net.Conn, p []byte) error {
	for len(p) > 0 {
		n, err := conn.Write(p)
		if err != nil {
			return err
		}
		p = p[n:]
	}
	return nil
}
