package gotests

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func isTransientNetworkErr(out string) bool {
	s := strings.ToLower(out)
	patterns := []string{
		"no such host", "timeout", "tempor", "connection reset", "connection refused", "429",
	}
	for _, p := range patterns {
		if strings.Contains(s, p) {
			return true
		}
	}
	return false
}

func TestLiveMSNScreener(t *testing.T) {
	if os.Getenv("RUN_LIVE_E2E") != "1" {
		t.Skip("set RUN_LIVE_E2E=1 to run live tests")
	}

	outFile := filepath.Join(repoRoot(), "output", "live_test_screener.json")
	code, out := runCLILive(t, nil,
		"msn", "screener",
		"--region", "id",
		"--filter", "large-cap",
		"--limit", "1",
		"--output", outFile,
	)
	if code != 0 {
		if isTransientNetworkErr(out) {
			t.Skipf("transient/live network issue: %s", out)
		}
		t.Fatalf("live screener failed: %s", out)
	}
}

func TestLiveNewsQuery(t *testing.T) {
	if os.Getenv("RUN_LIVE_E2E") != "1" {
		t.Skip("set RUN_LIVE_E2E=1 to run live tests")
	}
	if os.Getenv("BRAVE_API_KEY") == "" {
		t.Skip("BRAVE_API_KEY not set")
	}

	outFile := filepath.Join(repoRoot(), "output", "live_test_news.json")
	code, out := runCLILive(t, nil,
		"news", "IHSG",
		"--from", "2026-03-04",
		"--to", "2026-03-06",
		"--count", "1",
		"--concurrency", "1",
		"--output", outFile,
	)
	if code != 0 {
		if isTransientNetworkErr(out) {
			t.Skipf("transient/live network issue: %s", out)
		}
		t.Fatalf("live news failed: %s", out)
	}
}
