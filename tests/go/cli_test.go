package gotests

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

var (
	repoRootPath string
	testBinPath  string
)

func repoRoot() string {
	return repoRootPath
}

func TestMain(m *testing.M) {
	root, err := filepath.Abs("../..")
	if err != nil {
		fmt.Fprintf(os.Stderr, "resolve repo root: %v\n", err)
		os.Exit(1)
	}
	repoRootPath = root

	binDir := filepath.Join(root, ".bin")
	if err := os.MkdirAll(binDir, 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "create .bin dir: %v\n", err)
		os.Exit(1)
	}

	testBinPath = filepath.Join(binDir, "rubick-test")
	build := exec.Command("go", "build", "-o", testBinPath, "./cmd/rubick")
	build.Dir = root
	build.Stdout = os.Stdout
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "build test binary: %v\n", err)
		os.Exit(1)
	}

	code := m.Run()
	_ = os.Remove(testBinPath)
	os.Exit(code)
}

func runCLI(t *testing.T, args ...string) (int, string) {
	t.Helper()
	cmd := exec.Command(testBinPath, args...)
	cmd.Dir = repoRoot()

	var buf bytes.Buffer
	cmd.Stdout = &buf
	cmd.Stderr = &buf

	err := cmd.Run()
	if err == nil {
		return 0, buf.String()
	}
	if ee, ok := err.(*exec.ExitError); ok {
		return ee.ExitCode(), buf.String()
	}
	t.Fatalf("failed to run command %v: %v", args, err)
	return -1, ""
}

func runCLILive(t *testing.T, env map[string]string, args ...string) (int, string) {
	t.Helper()
	cmd := exec.Command(testBinPath, args...)
	cmd.Dir = repoRoot()
	cmd.Env = os.Environ()
	for k, v := range env {
		cmd.Env = append(cmd.Env, k+"="+v)
	}

	var buf bytes.Buffer
	cmd.Stdout = &buf
	cmd.Stderr = &buf

	err := cmd.Run()
	if err == nil {
		return 0, buf.String()
	}
	if ee, ok := err.(*exec.ExitError); ok {
		return ee.ExitCode(), buf.String()
	}
	t.Fatalf("failed to run command %v: %v", args, err)
	return -1, ""
}

func TestCLIHelpExitCodes(t *testing.T) {
	cases := [][]string{
		{"--help"},
		{"msn", "--help"},
		{"msn", "screener", "--help"},
		{"msn", "fetch", "--help"},
		{"msn", "fetch-all", "--help"},
		{"msn", "lookup", "--help"},
		{"news", "--help"},
		{"export", "--help"},
		{"extractor", "--help"},
	}

	for _, c := range cases {
		code, out := runCLI(t, c...)
		if code != 0 {
			t.Fatalf("expected exit 0 for %v, got %d\n%s", c, code, out)
		}
	}
}

func TestCLIErrorExitCodes(t *testing.T) {
	cases := [][]string{
		{"unknown"},
		{"msn"},
		{"news"},
		{"export"},
		{"extractor"},
	}

	for _, c := range cases {
		code, _ := runCLI(t, c...)
		if code == 0 {
			t.Fatalf("expected non-zero exit for %v", c)
		}
	}
}

func TestCLILookup(t *testing.T) {
	code, out := runCLI(t, "msn", "lookup", "BBCA")
	if code != 0 {
		t.Fatalf("expected success, got %d\n%s", code, out)
	}
	if !strings.Contains(out, "BBCA") {
		t.Fatalf("expected output to contain BBCA, got:\n%s", out)
	}
}
