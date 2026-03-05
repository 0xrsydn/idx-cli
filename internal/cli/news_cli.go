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
	"sync"
	"syscall"
	"time"

	"github.com/enetx/g"
	"github.com/enetx/surf"
	"github.com/joho/godotenv"
)

type NewsConfig struct {
	Query       string
	From        time.Time
	To          time.Time
	Count       int
	Concurrency int
	Output      string
	StockMode   bool
}

type EnrichedResult struct {
	Title         string `json:"title"`
	URL           string `json:"url"`
	Description   string `json:"description"`
	PageAge       string `json:"page_age"`
	Text          string `json:"text"`
	FetchStatus   string `json:"fetch_status"`
	ExtractStatus string `json:"extract_status"`
}

type OutputData struct {
	Query       string           `json:"query"`
	GeneratedAt string           `json:"generated_at"`
	Results     []EnrichedResult `json:"results"`
}

func runNewsCommand(args []string) int {
	godotenv.Load()

	if len(args) == 0 {
		printNewsUsage()
		return 1
	}
	if args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
		printNewsUsage()
		return 0
	}

	cfg, err := parseNewsArgs(args)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		return 1
	}

	if err := executeNews(cfg); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		return 1
	}
	return 0
}

func executeNews(config NewsConfig) error {
	finalQuery := config.Query
	if config.StockMode {
		terms := strings.Split(config.Query, ",")
		for i := range terms {
			terms[i] = strings.TrimSpace(terms[i])
		}
		finalQuery = BuildStockQuery(terms...)
		log.Printf("Stock mode query: %s", finalQuery)
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

	client := surf.NewClient().Builder().Impersonate().Chrome().Build().Unwrap()
	defer client.CloseIdleConnections()

	log.Printf("Searching for: %s", finalQuery)
	log.Printf("Date range: %s to %s, count: %d", config.From.Format("2006-01-02"), config.To.Format("2006-01-02"), config.Count)

	searchResults, err := SearchBrave(client, SearchConfig{Query: finalQuery, From: config.From, To: config.To, Count: config.Count})
	if err != nil {
		return fmt.Errorf("failed to search: %w", err)
	}
	log.Printf("Found %d results", len(searchResults))
	if len(searchResults) == 0 {
		log.Println("No results found, exiting")
		return nil
	}

	log.Println("Starting Python extractor...")
	extractor, err := NewExtractor(config.Concurrency)
	if err != nil {
		return fmt.Errorf("failed to start extractor: %w", err)
	}
	defer extractor.Close()
	log.Println("Python extractor ready")

	results := processURLs(ctx, client, extractor, searchResults, config.Concurrency)
	output := OutputData{Query: finalQuery, GeneratedAt: time.Now().UTC().Format(time.RFC3339), Results: results}
	if err := saveOutput(output, config.Output); err != nil {
		return fmt.Errorf("failed to save output: %w", err)
	}

	successCount := 0
	for _, r := range results {
		if r.ExtractStatus == "ok" {
			successCount++
		}
	}
	log.Printf("Output saved to %s", config.Output)
	log.Printf("Successfully extracted %d/%d articles", successCount, len(results))
	return nil
}

func printNewsUsage() {
	fmt.Fprintf(os.Stderr, `Usage: rubick news <query> [options]

Arguments:
  <query>              Search query (required)
                       For --stock mode: comma-separated stock terms

Options:
  --from <date>        Start date in YYYY-MM-DD format (default: 7 days ago)
  --to <date>          End date in YYYY-MM-DD format (default: today)
  --count <n>          Number of results to fetch (default: 20)
  --concurrency <n>    Number of parallel workers (default: 10)
  --output, -o <file>  Output file path (default: output_YYYYMMDD.json)
  --stock              Auto-builds IDX-focused boolean query

Environment:
  BRAVE_API_KEY        Brave Search API key

Examples:
  rubick news "IHSG stock market"
  rubick news "BBCA,Bank Central Asia" --stock --from 2026-02-01 --to 2026-02-10
`)
}

func parseNewsArgs(args []string) (NewsConfig, error) {
	query := args[0]
	args = args[1:]

	now := time.Now()
	cfg := NewsConfig{
		Query:       query,
		From:        now.AddDate(0, 0, -7),
		To:          now,
		Count:       20,
		Concurrency: 10,
		Output:      fmt.Sprintf("output_%s.json", now.Format("20060102")),
	}

	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--from":
			v, n, err := requireValue(args, i, "--from")
			if err != nil {
				return cfg, err
			}
			t, err := time.Parse("2006-01-02", v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --from date: %w", err)
			}
			cfg.From = t
			i = n
		case "--to":
			v, n, err := requireValue(args, i, "--to")
			if err != nil {
				return cfg, err
			}
			t, err := time.Parse("2006-01-02", v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --to date: %w", err)
			}
			cfg.To = t
			i = n
		case "--count":
			v, n, err := requireValue(args, i, "--count")
			if err != nil {
				return cfg, err
			}
			nval, err := strconv.Atoi(v)
			if err != nil {
				return cfg, fmt.Errorf("invalid --count value: %w", err)
			}
			cfg.Count = nval
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
		case "--output", "-o":
			v, n, err := requireValue(args, i, "--output")
			if err != nil {
				return cfg, err
			}
			cfg.Output = v
			i = n
		case "--stock":
			cfg.StockMode = true
		default:
			return cfg, fmt.Errorf("unknown option: %s", args[i])
		}
	}

	if cfg.Count < 1 {
		return cfg, fmt.Errorf("--count must be >= 1")
	}
	if cfg.Concurrency < 1 {
		return cfg, fmt.Errorf("--concurrency must be >= 1")
	}
	if cfg.From.After(cfg.To) {
		return cfg, fmt.Errorf("--from must be on or before --to")
	}

	return cfg, nil
}

func processURLs(ctx context.Context, client *surf.Client, extractor *Extractor, searchResults []BraveResult, concurrency int) []EnrichedResult {
	results := make([]EnrichedResult, len(searchResults))
	for i, sr := range searchResults {
		results[i] = EnrichedResult{Title: sr.Title, URL: sr.URL, Description: sr.Description, PageAge: sr.PageAge, FetchStatus: "pending", ExtractStatus: "pending"}
	}

	work := make(chan int, len(searchResults))
	for i := range searchResults {
		work <- i
	}
	close(work)

	var wg sync.WaitGroup
	var mu sync.Mutex
	for w := 0; w < concurrency; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				select {
				case <-ctx.Done():
					return
				case idx, ok := <-work:
					if !ok {
						return
					}
					result := processURL(ctx, client, extractor, searchResults[idx])
					mu.Lock()
					results[idx] = result
					mu.Unlock()
					log.Printf("[%d/%d] %s - fetch: %s, extract: %s", idx+1, len(searchResults), truncate(result.URL, 50), result.FetchStatus, result.ExtractStatus)
				}
			}
		}()
	}
	wg.Wait()
	return results
}

func processURL(ctx context.Context, client *surf.Client, extractor *Extractor, sr BraveResult) EnrichedResult {
	result := EnrichedResult{Title: sr.Title, URL: sr.URL, Description: sr.Description, PageAge: sr.PageAge}
	if err := ctx.Err(); err != nil {
		result.FetchStatus = "cancelled"
		result.ExtractStatus = "skipped"
		return result
	}
	resp := client.Get(g.String(sr.URL)).Do()
	if resp.IsErr() {
		result.FetchStatus = "failed"
		result.ExtractStatus = "skipped"
		return result
	}
	r := resp.Ok()
	if r.StatusCode != 200 {
		result.FetchStatus = "failed"
		result.ExtractStatus = "skipped"
		return result
	}
	html := r.Body.String().Ok().Std()
	result.FetchStatus = "ok"
	extractResp, err := extractor.Extract(ctx, sr.URL, html)
	if err != nil {
		result.ExtractStatus = "failed"
		return result
	}
	result.Text = extractResp.Text
	result.ExtractStatus = extractResp.Status
	return result
}

func saveOutput(output OutputData, filename string) error {
	data, err := json.MarshalIndent(output, "", "  ")
	if err != nil {
		return err
	}
	if dir := filepath.Dir(filename); dir != "." {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
	}
	return os.WriteFile(filename, data, 0o644)
}

func truncate(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	return s[:maxLen-3] + "..."
}
