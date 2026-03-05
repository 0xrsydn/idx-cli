package cli

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
)

func Run(args []string) int {
	if len(args) == 0 {
		printRootUsage()
		return 1
	}

	if args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		printRootUsage()
		return 0
	}

	switch args[0] {
	case "msn":
		return runMSNCommand(args[1:])
	case "news":
		return runNewsCommand(args[1:])
	case "export":
		return runExportCommand(args[1:])
	case "extractor":
		return runExtractorCommand(args[1:])
	default:
		printRootUsage()
		fmt.Fprintf(os.Stderr, "\nerror: unknown command: %s\n", args[0])
		return 1
	}
}

func printRootUsage() {
	fmt.Fprintf(os.Stderr, `Rubick - Unified Market Intelligence CLI

Usage:
  rubick <command> [options]

Commands:
  msn        MSN finance workflows (screener, fetch, fetch-all, lookup)
  news       Brave news search + Python text extraction
  export     Python export tools (dashboard/history/simple)
  extractor  Run raw Python extractor server (advanced)

Examples:
  # News mode with explicit command
  rubick news "BBCA,Bank Central Asia" --stock --count 20

  # MSN screener
  rubick msn screener --region id --filter top-performers --limit 20 -o output/screener.json

  # MSN fetch-all to SQLite
  rubick msn fetch-all --index idx30 --db output/stocks.db --concurrency 3

  # Export dashboard from SQLite
  rubick export dashboard --db output/stocks.db --output output/dashboard.xlsx

  # Export history workbook from SQLite
  rubick export history --db output/stocks.db --output output/history.xlsx

  # Export simple tables (json/csv/xlsx) from SQLite
  rubick export simple --db output/stocks.db --format csv --output output/simple_csv

  # Run extractor server directly
  rubick extractor --socket /tmp/extractor.sock
`)
}

func runExportCommand(args []string) int {
	if len(args) == 0 {
		fmt.Fprintf(os.Stderr, `Usage: rubick export <dashboard|history|simple> [script options]

Examples:
  rubick export dashboard --db output/stocks.db --output output/dashboard.xlsx
  rubick export history --db output/stocks.db --output output/history.xlsx
  rubick export simple --db output/stocks.db --format csv --output output/csv/
`)
		return 1
	}

	if args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		fmt.Fprintf(os.Stderr, `Usage: rubick export <dashboard|history|simple> [script options]

Examples:
  rubick export dashboard --db output/stocks.db --output output/dashboard.xlsx
  rubick export history --db output/stocks.db --output output/history.xlsx
  rubick export simple --db output/stocks.db --format csv --output output/csv/
`)
		return 0
	}

	script := ""
	switch args[0] {
	case "dashboard":
		script = "scripts/export_dashboard.py"
	case "history":
		script = "scripts/export_history.py"
	case "simple":
		script = "scripts/export_simple.py"
	default:
		fmt.Fprintf(os.Stderr, "Unknown export target: %s\n", args[0])
		return 1
	}

	cmdArgs := append([]string{"run", "python", script}, args[1:]...)
	return runPassthrough("uv", cmdArgs)
}

func runExtractorCommand(args []string) int {
	if len(args) == 0 {
		fmt.Fprintf(os.Stderr, "Usage: rubick extractor --socket /tmp/extractor.sock\n")
		return 1
	}

	if args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		fmt.Fprintf(os.Stderr, "Usage: rubick extractor --socket /tmp/extractor.sock\n")
		return 0
	}

	cmdArgs := append([]string{"run", "python", "extractor.py"}, args...)
	return runPassthrough("uv", cmdArgs)
}

func runPassthrough(bin string, args []string) int {
	cmd := exec.Command(bin, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin

	if err := cmd.Run(); err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			return exitErr.ExitCode()
		}
		fmt.Fprintf(os.Stderr, "failed to run %s: %v\n", bin, err)
		return 1
	}
	return 0
}
