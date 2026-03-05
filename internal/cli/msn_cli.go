package cli

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"rubick/msn"
)

func runMSNCommand(args []string) int {
	if len(args) == 0 {
		printMSNUsage()
		return 1
	}

	if args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		printMSNUsage()
		return 0
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)
	defer signal.Stop(sigChan)
	go func() {
		<-sigChan
		log.Println("Shutting down...")
		cancel()
	}()

	if err := executeMSNCommand(ctx, args); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		return 1
	}
	return 0
}

func executeMSNCommand(ctx context.Context, args []string) error {
	subcommand := args[0]
	subArgs := args[1:]

	switch subcommand {
	case "screener":
		return runScreener(ctx, subArgs)
	case "fetch":
		return runFetch(ctx, subArgs)
	case "fetch-all":
		return runFetchAll(ctx, subArgs)
	case "lookup":
		return runLookup(subArgs)
	default:
		printMSNUsage()
		return fmt.Errorf("unknown msn subcommand: %s", subcommand)
	}
}

func printMSNUsage() {
	fmt.Fprintf(os.Stderr, `MSN Stock Scraper - Fetch Indonesian stock data from MSN Finance

Usage:
  rubick msn <command> [options]

Commands:
  screener    Run stock screener to find stocks by criteria
  fetch       Fetch comprehensive data for specific stocks
  fetch-all   Fetch ALL Indonesian stocks to SQLite database
  lookup      Look up MSN ID for ticker symbols

Screener:
  rubick msn screener --region id --filter top-performers --limit 20 -o stocks.json

Fetch:
  rubick msn fetch --tickers BBCA,BBRI,TLKM -o bank_stocks.json
  rubick msn fetch --input stocks.json -o full_data.json

Fetch-All:
  rubick msn fetch-all --index idx30 --db output/stocks.db --concurrency 3

Lookup:
  rubick msn lookup BBCA BBRI TLKM
`)
}

// Screener

type ScreenerCLIConfig struct {
	Region string
	Filter string
	Limit  int
	Output string
}

func parseScreenerArgs(args []string) (ScreenerCLIConfig, error) {
	cfg := ScreenerCLIConfig{
		Region: "id",
		Filter: "large-cap",
		Limit:  50,
		Output: fmt.Sprintf("screener_%s.json", time.Now().Format("20060102")),
	}

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--region":
			v, n, err := requireValue(args, i, "--region")
			if err != nil {
				return cfg, err
			}
			cfg.Region = v
			i = n
		case "--filter":
			v, n, err := requireValue(args, i, "--filter")
			if err != nil {
				return cfg, err
			}
			cfg.Filter = v
			i = n
		case "--limit":
			v, n, err := requireValue(args, i, "--limit")
			if err != nil {
				return cfg, err
			}
			limit, err := strconv.Atoi(v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --limit value: %w", err)
			}
			cfg.Limit = limit
			i = n
		case "--output", "-o":
			v, n, err := requireValue(args, i, "--output")
			if err != nil {
				return cfg, err
			}
			cfg.Output = v
			i = n
		default:
			return cfg, fmt.Errorf("unknown option: %s", args[i])
		}
	}
	if cfg.Limit < 1 {
		return cfg, fmt.Errorf("--limit must be >= 1")
	}
	return cfg, nil
}

func runScreener(ctx context.Context, args []string) error {
	if wantsHelp(args) {
		printMSNScreenerUsage()
		return nil
	}

	cfg, err := parseScreenerArgs(args)
	if err != nil {
		return err
	}

	filter, err := msn.ParseScreenerFilter(cfg.Filter)
	if err != nil {
		return fmt.Errorf("invalid filter: %w", err)
	}

	log.Printf("Running screener: region=%s filter=%s limit=%d", cfg.Region, cfg.Filter, cfg.Limit)
	client := msn.NewMSNClient()
	defer client.Close()

	result, err := client.RunScreener(msn.ScreenerConfig{Region: cfg.Region, Filter: filter, Limit: cfg.Limit})
	if err != nil {
		return fmt.Errorf("screener failed: %w", err)
	}

	output := msn.ScreenerOutput{
		Filter:      cfg.Filter,
		Region:      cfg.Region,
		GeneratedAt: time.Now().UTC().Format(time.RFC3339),
		Total:       result.Total,
		Stocks:      result.Value,
	}
	if err := saveJSON(output, cfg.Output); err != nil {
		return fmt.Errorf("failed to save output: %w", err)
	}

	log.Printf("Output saved to %s", cfg.Output)
	fmt.Println("\nTop 10 results:")
	for i, stock := range result.Value {
		if i >= 10 {
			break
		}
		fmt.Printf("  %s (%s): %.2f (%.2f%%)\n", stock.Symbol, stock.ID, stock.Price, stock.PriceChangePct)
	}
	return nil
}

// Fetch

type FetchCLIConfig struct {
	Input       string
	IDs         []string
	Tickers     []string
	Concurrency int
	Output      string
}

func parseFetchArgs(args []string) (FetchCLIConfig, error) {
	cfg := FetchCLIConfig{Concurrency: 5, Output: fmt.Sprintf("stocks_%s.json", time.Now().Format("20060102"))}

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--input":
			v, n, err := requireValue(args, i, "--input")
			if err != nil {
				return cfg, err
			}
			cfg.Input = v
			i = n
		case "--ids":
			v, n, err := requireValue(args, i, "--ids")
			if err != nil {
				return cfg, err
			}
			cfg.IDs = appendCSV(cfg.IDs, v, false)
			i = n
		case "--tickers":
			v, n, err := requireValue(args, i, "--tickers")
			if err != nil {
				return cfg, err
			}
			cfg.Tickers = appendCSV(cfg.Tickers, v, true)
			i = n
		case "--concurrency":
			v, n, err := requireValue(args, i, "--concurrency")
			if err != nil {
				return cfg, err
			}
			conc, err := strconv.Atoi(v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --concurrency value: %w", err)
			}
			cfg.Concurrency = conc
			i = n
		case "--output", "-o":
			v, n, err := requireValue(args, i, "--output")
			if err != nil {
				return cfg, err
			}
			cfg.Output = v
			i = n
		default:
			return cfg, fmt.Errorf("unknown option: %s", args[i])
		}
	}
	if cfg.Concurrency < 1 {
		return cfg, fmt.Errorf("--concurrency must be >= 1")
	}

	return cfg, nil
}

func runFetch(ctx context.Context, args []string) error {
	if wantsHelp(args) {
		printMSNFetchUsage()
		return nil
	}

	cfg, err := parseFetchArgs(args)
	if err != nil {
		return err
	}

	ids := make([]string, 0)
	if cfg.Input != "" {
		inputIDs, err := readScreenerOutput(cfg.Input)
		if err != nil {
			return fmt.Errorf("failed to read input file: %w", err)
		}
		ids = append(ids, inputIDs...)
	}
	ids = append(ids, cfg.IDs...)

	for _, ticker := range cfg.Tickers {
		id := msn.GetIDXStockID(ticker)
		if id == "" {
			log.Printf("Warning: Unknown ticker '%s', skipping", ticker)
			continue
		}
		log.Printf("Resolved %s -> %s", ticker, id)
		ids = append(ids, id)
	}

	if len(ids) == 0 {
		return fmt.Errorf("no stock IDs provided. use --input, --ids, or --tickers")
	}

	ids = dedupe(ids)
	log.Printf("Fetching data for %d stocks with concurrency %d", len(ids), cfg.Concurrency)

	fetcher := msn.NewStockFetcher()
	defer fetcher.Close()

	stocks := fetcher.FetchStocks(ctx, ids, cfg.Concurrency)
	output := msn.FetchOutput{GeneratedAt: time.Now().UTC().Format(time.RFC3339), Total: len(stocks), Stocks: stocks}
	if err := saveJSON(output, cfg.Output); err != nil {
		return fmt.Errorf("failed to save output: %w", err)
	}

	successCount := 0
	for _, stock := range stocks {
		apiSuccess := 0
		for _, status := range stock.FetchStatus {
			if status == "ok" {
				apiSuccess++
			}
		}
		if apiSuccess > 0 {
			successCount++
		}
	}
	log.Printf("Output saved to %s", cfg.Output)
	log.Printf("Successfully fetched %d/%d stocks", successCount, len(stocks))
	return nil
}

func readScreenerOutput(filename string) ([]string, error) {
	data, err := os.ReadFile(filename)
	if err != nil {
		return nil, err
	}

	var output msn.ScreenerOutput
	if err := json.Unmarshal(data, &output); err != nil {
		return nil, err
	}

	ids := make([]string, len(output.Stocks))
	for i, stock := range output.Stocks {
		ids[i] = stock.ID
	}
	return ids, nil
}

func saveJSON(data any, filename string) error {
	jsonData, err := json.MarshalIndent(data, "", "  ")
	if err != nil {
		return err
	}
	if dir := filepath.Dir(filename); dir != "." {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
	}
	return os.WriteFile(filename, jsonData, 0o644)
}

func runLookup(args []string) error {
	if wantsHelp(args) {
		printMSNLookupUsage()
		return nil
	}

	if len(args) == 0 {
		return fmt.Errorf("usage: rubick msn lookup <ticker1> [ticker2] ...")
	}

	fmt.Printf("%-8s %-10s %s\n", "Ticker", "MSN ID", "Company Name")
	fmt.Println(strings.Repeat("-", 50))

	found := 0
	for _, ticker := range args {
		ticker = strings.ToUpper(strings.TrimSpace(ticker))
		stock, ok := msn.GetIDXStock(ticker)
		if ok {
			fmt.Printf("%-8s %-10s %s\n", ticker, stock.ID, stock.Name)
			found++
		} else {
			fmt.Printf("%-8s %-10s %s\n", ticker, "-", "(not found)")
		}
	}
	fmt.Println(strings.Repeat("-", 50))
	fmt.Printf("Found %d/%d tickers\n", found, len(args))
	return nil
}

// Fetch-all

type FetchAllConfig struct {
	DB          string
	Index       string
	Proxy       string
	Concurrency int
	RPS         float64
	MinDelayMs  int
	MaxDelayMs  int
	Retry       int
	Limit       int
	Resume      bool
}

func parseFetchAllArgs(args []string) (FetchAllConfig, error) {
	cfg := FetchAllConfig{
		DB:          "output/stocks.db",
		Index:       "all",
		Concurrency: 5,
		RPS:         25,
		MinDelayMs:  100,
		MaxDelayMs:  500,
		Retry:       2,
	}

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--db":
			v, n, err := requireValue(args, i, "--db")
			if err != nil {
				return cfg, err
			}
			cfg.DB = v
			i = n
		case "--index":
			v, n, err := requireValue(args, i, "--index")
			if err != nil {
				return cfg, err
			}
			cfg.Index = strings.ToLower(v)
			i = n
		case "--proxy":
			v, n, err := requireValue(args, i, "--proxy")
			if err != nil {
				return cfg, err
			}
			cfg.Proxy = v
			i = n
		case "--concurrency":
			v, n, err := requireValue(args, i, "--concurrency")
			if err != nil {
				return cfg, err
			}
			nval, err := strconv.Atoi(v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --concurrency value: %w", err)
			}
			cfg.Concurrency = nval
			i = n
		case "--rps":
			v, n, err := requireValue(args, i, "--rps")
			if err != nil {
				return cfg, err
			}
			fval, err := strconv.ParseFloat(v, 64)
			if err != nil {
				return cfg, fmt.Errorf("invalid --rps value: %w", err)
			}
			cfg.RPS = fval
			i = n
		case "--delay":
			v, n, err := requireValue(args, i, "--delay")
			if err != nil {
				return cfg, err
			}
			minDelay, maxDelay, err := parseDelay(v)
			if err != nil {
				return cfg, err
			}
			cfg.MinDelayMs, cfg.MaxDelayMs = minDelay, maxDelay
			i = n
		case "--retry":
			v, n, err := requireValue(args, i, "--retry")
			if err != nil {
				return cfg, err
			}
			nval, err := strconv.Atoi(v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --retry value: %w", err)
			}
			cfg.Retry = nval
			i = n
		case "--limit":
			v, n, err := requireValue(args, i, "--limit")
			if err != nil {
				return cfg, err
			}
			nval, err := strconv.Atoi(v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --limit value: %w", err)
			}
			cfg.Limit = nval
			i = n
		case "--resume":
			cfg.Resume = true
		default:
			return cfg, fmt.Errorf("unknown option: %s", args[i])
		}
	}
	if cfg.Concurrency < 1 {
		return cfg, fmt.Errorf("--concurrency must be >= 1")
	}
	if cfg.RPS <= 0 {
		return cfg, fmt.Errorf("--rps must be > 0")
	}
	if cfg.MinDelayMs < 0 || cfg.MaxDelayMs < 0 || cfg.MinDelayMs > cfg.MaxDelayMs {
		return cfg, fmt.Errorf("--delay must satisfy 0 <= min <= max")
	}
	if cfg.Retry < 0 {
		return cfg, fmt.Errorf("--retry must be >= 0")
	}
	if cfg.Limit < 0 {
		return cfg, fmt.Errorf("--limit must be >= 0")
	}
	return cfg, nil
}

func runFetchAll(ctx context.Context, args []string) error {
	if wantsHelp(args) {
		printMSNFetchAllUsage()
		return nil
	}

	cfg, err := parseFetchAllArgs(args)
	if err != nil {
		return err
	}

	log.Printf("Fetch-All Configuration:")
	log.Printf("  Database: %s", cfg.DB)
	log.Printf("  Index: %s", cfg.Index)
	log.Printf("  Concurrency: %d workers", cfg.Concurrency)
	log.Printf("  Rate limit: %.1f req/sec", cfg.RPS)
	log.Printf("  Delay: %d-%d ms", cfg.MinDelayMs, cfg.MaxDelayMs)
	log.Printf("  Retry: %d attempts", cfg.Retry)
	if cfg.Proxy != "" {
		log.Printf("  Proxy: %s", cfg.Proxy)
	}
	if cfg.Limit > 0 {
		log.Printf("  Limit: %d stocks", cfg.Limit)
	}
	if cfg.Resume {
		log.Printf("  Resume: enabled")
	}

	db, err := msn.NewStockDB(cfg.DB)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer db.Close()

	stocks := getStocksByIndex(cfg.Index)
	if cfg.Limit > 0 && cfg.Limit < len(stocks) {
		limited := make(map[string]msn.IDXStock)
		count := 0
		for ticker, stock := range stocks {
			if count >= cfg.Limit {
				break
			}
			limited[ticker] = stock
			count++
		}
		stocks = limited
	}

	log.Printf("Stock list: %d stocks from '%s' index", len(stocks), cfg.Index)

	var runID int64
	if cfg.Resume {
		runID, err = db.GetLastRunID()
		if err != nil {
			return fmt.Errorf("failed to get last run: %w", err)
		}
		if runID > 0 {
			log.Printf("Resuming run #%d", runID)
		} else {
			log.Printf("No incomplete run found, starting fresh")
			cfg.Resume = false
		}
	}

	if !cfg.Resume {
		cfgMap := map[string]any{
			"index":       cfg.Index,
			"concurrency": cfg.Concurrency,
			"rps":         cfg.RPS,
			"delay":       fmt.Sprintf("%d-%d", cfg.MinDelayMs, cfg.MaxDelayMs),
			"retry":       cfg.Retry,
			"proxy":       cfg.Proxy != "",
		}
		runID, err = db.StartScrapeRun(len(stocks), cfgMap)
		if err != nil {
			return fmt.Errorf("failed to start run: %w", err)
		}
		log.Printf("Started run #%d", runID)
		if err := db.InitProgress(runID, stocks); err != nil {
			return fmt.Errorf("failed to init progress: %w", err)
		}
	}

	pendingStocks, err := db.GetPendingStocks(runID)
	if err != nil {
		return fmt.Errorf("failed to get pending stocks: %w", err)
	}
	log.Printf("Pending: %d stocks to process", len(pendingStocks))
	if len(pendingStocks) == 0 {
		log.Println("No pending stocks, run complete")
		return nil
	}

	rateLimiter := msn.NewRateLimiter(msn.RateLimiterConfig{RequestsPerSecond: cfg.RPS, MinDelayMs: cfg.MinDelayMs, MaxDelayMs: cfg.MaxDelayMs})
	client := msn.NewMSNClientWithConfig(msn.MSNClientConfig{Proxy: cfg.Proxy, RateLimiter: rateLimiter})
	defer client.Close()

	type workItem struct{ ID, Ticker string }
	workChan := make(chan workItem, len(pendingStocks))
	for _, s := range pendingStocks {
		workChan <- workItem{ID: s.ID, Ticker: s.Ticker}
	}
	close(workChan)

	var processed, successful, failed int
	total := len(pendingStocks)
	startTime := time.Now()

	statusTicker := time.NewTicker(5 * time.Second)
	defer statusTicker.Stop()
	go func() {
		for range statusTicker.C {
			elapsed := time.Since(startTime)
			rate := float64(processed) / elapsed.Seconds()
			remaining := total - processed
			eta := time.Duration(float64(remaining)/rate) * time.Second
			log.Printf("Progress: %d/%d (%.1f%%) | Success: %d | Failed: %d | Rate: %.1f/s | ETA: %s",
				processed, total, float64(processed)*100/float64(total), successful, failed, rate, eta.Round(time.Second))
		}
	}()

	done := make(chan bool)
	results := make(chan struct {
		ticker  string
		success bool
		apis    int
		err     string
	}, cfg.Concurrency)

	for w := 0; w < cfg.Concurrency; w++ {
		go func() {
			for {
				select {
				case <-ctx.Done():
					return
				case work, ok := <-workChan:
					if !ok {
						return
					}
					db.MarkProgressStarted(runID, work.ID)
					var stockData *msn.StockData
					var fetchErr error
					for attempt := 0; attempt <= cfg.Retry; attempt++ {
						stockData, fetchErr = client.FetchStockData(work.ID)
						if fetchErr == nil {
							break
						}
						if attempt < cfg.Retry {
							time.Sleep(time.Duration(500*(attempt+1)) * time.Millisecond)
						}
					}
					apisSuccess, apisFailed := 0, 0
					if stockData != nil {
						for _, status := range stockData.FetchStatus {
							if status == "ok" {
								apisSuccess++
							} else {
								apisFailed++
							}
						}
						if err := db.SaveStockData(stockData); err != nil {
							fetchErr = fmt.Errorf("save failed: %w", err)
						}
					}
					status := "success"
					errMsg := ""
					if fetchErr != nil || apisSuccess == 0 {
						status = "failed"
						if fetchErr != nil {
							errMsg = fetchErr.Error()
						}
					}
					db.UpdateProgress(runID, work.ID, status, apisSuccess, apisFailed, errMsg)
					results <- struct {
						ticker  string
						success bool
						apis    int
						err     string
					}{work.Ticker, status == "success", apisSuccess, errMsg}
				}
			}
		}()
	}

	go func() {
		for processed < total {
			select {
			case <-ctx.Done():
				done <- false
				return
			case r := <-results:
				processed++
				if r.success {
					successful++
					log.Printf("[%d/%d] %s - %d APIs succeeded", processed, total, r.ticker, r.apis)
				} else {
					failed++
					log.Printf("[%d/%d] %s - FAILED: %s", processed, total, r.ticker, r.err)
				}
			}
		}
		done <- true
	}()

	completed := <-done
	elapsed := time.Since(startTime)
	if completed {
		db.CompleteScrapeRun(runID, "completed")
		log.Printf("\n=== Run #%d Completed ===", runID)
	} else {
		db.CompleteScrapeRun(runID, "interrupted")
		log.Printf("\n=== Run #%d Interrupted ===", runID)
	}
	log.Printf("Total: %d | Success: %d | Failed: %d", processed, successful, failed)
	log.Printf("Duration: %s | Rate: %.1f stocks/sec", elapsed.Round(time.Second), float64(processed)/elapsed.Seconds())
	log.Printf("Database: %s", cfg.DB)

	staleStocks, _ := db.GetStaleStocks(7)
	if len(staleStocks) > 0 {
		log.Printf("\nWarning: %d stocks not seen in 7+ days:", len(staleStocks))
		for i, ticker := range staleStocks {
			if i >= 10 {
				log.Printf("  ... and %d more", len(staleStocks)-10)
				break
			}
			log.Printf("  - %s", ticker)
		}
	}
	return nil
}

// Helpers

func requireValue(args []string, i int, flag string) (string, int, error) {
	if i+1 >= len(args) {
		return "", i, fmt.Errorf("%s requires a value", flag)
	}
	return args[i+1], i + 1, nil
}

func appendCSV(dst []string, csv string, upper bool) []string {
	for _, part := range strings.Split(csv, ",") {
		v := strings.TrimSpace(part)
		if upper {
			v = strings.ToUpper(v)
		}
		if v != "" {
			dst = append(dst, v)
		}
	}
	return dst
}

func dedupe(values []string) []string {
	seen := make(map[string]bool, len(values))
	out := make([]string, 0, len(values))
	for _, v := range values {
		if !seen[v] {
			seen[v] = true
			out = append(out, v)
		}
	}
	return out
}

func parseDelay(v string) (int, int, error) {
	parts := strings.Split(v, "-")
	if len(parts) != 2 {
		return 0, 0, fmt.Errorf("invalid --delay value (use format min-max)")
	}
	minDelay, err := strconv.Atoi(parts[0])
	if err != nil {
		return 0, 0, fmt.Errorf("invalid --delay min value: %w", err)
	}
	maxDelay, err := strconv.Atoi(parts[1])
	if err != nil {
		return 0, 0, fmt.Errorf("invalid --delay max value: %w", err)
	}
	if minDelay > maxDelay {
		return 0, 0, fmt.Errorf("invalid --delay value: min must be <= max")
	}
	return minDelay, maxDelay, nil
}

func wantsHelp(args []string) bool {
	for _, a := range args {
		if a == "-h" || a == "--help" || a == "help" {
			return true
		}
	}
	return false
}

func printMSNScreenerUsage() {
	fmt.Fprintf(os.Stderr, `Usage: rubick msn screener [options]

Options:
  --region <code>     Country code (default: id)
  --filter <preset>   top-performers|worst-performers|high-dividend|low-pe|52w-high|52w-low|high-volume|large-cap
  --limit <n>         Max results (default: 50)
  --output, -o <file> Output JSON path
`)
}

func printMSNFetchUsage() {
	fmt.Fprintf(os.Stderr, `Usage: rubick msn fetch [options]

Options:
  --input <file>          Screener JSON input
  --ids <id1,id2,...>     Comma-separated MSN IDs
  --tickers <T1,T2,...>   Comma-separated ticker symbols
  --concurrency <n>       Parallel workers (default: 5)
  --output, -o <file>     Output JSON path
`)
}

func printMSNFetchAllUsage() {
	fmt.Fprintf(os.Stderr, `Usage: rubick msn fetch-all [options]

Options:
  --db <file>            SQLite database path (default: output/stocks.db)
  --index <name>         all|lq45|idx30|idx80 (default: all)
  --proxy <url>          Proxy URL (http://, https://, socks5://)
  --concurrency <n>      Parallel workers (default: 5)
  --rps <n>              Max requests/sec (default: 25)
  --delay <min-max>      Random delay ms (default: 100-500)
  --retry <n>            Retry attempts (default: 2)
  --limit <n>            Process only N stocks
  --resume               Resume incomplete run
`)
}

func printMSNLookupUsage() {
	fmt.Fprintf(os.Stderr, `Usage: rubick msn lookup <ticker1> [ticker2] ...
`)
}

func getStocksByIndex(index string) map[string]msn.IDXStock {
	allStocks := msn.GetAllIDXStocks()
	switch index {
	case "all":
		return allStocks
	case "lq45":
		return filterStocks(allStocks, []string{
			"ACES", "ADRO", "AKRA", "AMMN", "AMRT", "ANTM", "ASII", "BBCA",
			"BBNI", "BBRI", "BBTN", "BMRI", "BRPT", "BUKA", "CPIN", "EMTK",
			"ESSA", "EXCL", "GGRM", "GOTO", "HRUM", "ICBP", "INCO", "INDF",
			"INKP", "INTP", "ITMG", "KLBF", "MAPI", "MBMA", "MDKA", "MEDC",
			"PGAS", "PGEO", "PTBA", "SIDO", "SMGR", "TBIG", "TINS", "TLKM",
			"TOWR", "UNTR", "UNVR", "WIKA",
		})
	case "idx30":
		return filterStocks(allStocks, []string{
			"ADRO", "AMRT", "ANTM", "ASII", "BBCA", "BBNI", "BBRI", "BMRI",
			"BRPT", "CPIN", "EMTK", "EXCL", "GOTO", "ICBP", "INCO", "INDF",
			"ITMG", "KLBF", "MDKA", "MEDC", "PGAS", "PTBA", "SMGR", "TBIG",
			"TINS", "TLKM", "TOWR", "UNTR", "UNVR",
		})
	case "idx80":
		return filterStocks(allStocks, []string{
			"ACES", "ADRO", "AGII", "AKRA", "AMMN", "AMRT", "ANTM", "ARTO",
			"ASII", "BBCA", "BBNI", "BBRI", "BBTN", "BFIN", "BMRI", "BRPT",
			"BSDE", "BTPS", "BUKA", "CPIN", "CTRA", "DMAS", "EMTK", "ERAA",
			"ESSA", "EXCL", "GGRM", "GOTO", "HEAL", "HMSP", "HRUM", "ICBP",
			"INCO", "INDF", "INKP", "INTP", "ITMG", "JPFA", "JSMR", "KLBF",
			"LPKR", "LPPF", "MAPI", "MBMA", "MDKA", "MEDC", "MIKA", "MNCN",
			"PGAS", "PGEO", "PNBN", "PTBA", "PTPP", "PWON", "SCMA", "SIDO",
			"SMGR", "SMRA", "SRTG", "TAPG", "TBIG", "TINS", "TKIM", "TLKM",
			"TOWR", "TPIA", "UNTR", "UNVR", "WIKA", "WSKT",
		})
	default:
		log.Printf("Unknown index '%s', using all stocks", index)
		return allStocks
	}
}

func filterStocks(all map[string]msn.IDXStock, tickers []string) map[string]msn.IDXStock {
	result := make(map[string]msn.IDXStock)
	for _, ticker := range tickers {
		if stock, ok := all[ticker]; ok {
			result[ticker] = stock
		}
	}
	return result
}
