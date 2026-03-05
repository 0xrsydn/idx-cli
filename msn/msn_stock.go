package msn

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"
)

// StockFetcher handles parallel fetching of stock data
type StockFetcher struct {
	msnClient  *MSNClient
	bingClient *BingClient
}

// NewStockFetcher creates a new stock fetcher
func NewStockFetcher() *StockFetcher {
	return &StockFetcher{
		msnClient:  NewMSNClient(),
		bingClient: NewBingClient(),
	}
}

// Close closes all clients
func (f *StockFetcher) Close() {
	f.msnClient.Close()
	f.bingClient.Close()
}

// FetchResult holds the result of fetching a single stock
type StockFetchResult struct {
	Index int
	Stock *StockData
	Error error
}

// FetchStocks fetches data for multiple stocks in parallel
func (f *StockFetcher) FetchStocks(ctx context.Context, ids []string, concurrency int) []StockData {
	if concurrency <= 0 {
		concurrency = 5
	}

	results := make([]StockData, len(ids))

	// Create work channel
	work := make(chan int, len(ids))
	for i := range ids {
		work <- i
	}
	close(work)

	// Worker pool
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

					id := ids[idx]
					stock := f.fetchSingleStock(ctx, id)

					mu.Lock()
					results[idx] = *stock
					mu.Unlock()

					// Count successful fetches
					successCount := 0
					for k, v := range stock.FetchStatus {
						if v == "ok" {
							successCount++
						}
						_ = k
					}

					log.Printf("[%d/%d] %s (%s) - %d/%d APIs succeeded",
						idx+1, len(ids),
						stock.Ticker,
						stock.ID,
						successCount,
						len(stock.FetchStatus),
					)
				}
			}
		}()
	}

	wg.Wait()
	return results
}

// fetchSingleStock fetches all data for a single stock
func (f *StockFetcher) fetchSingleStock(ctx context.Context, id string) *StockData {
	stock := &StockData{
		ID:          id,
		FetchedAt:   time.Now().UTC().Format(time.RFC3339),
		FetchStatus: make(map[string]string),
		Charts:      make(map[string][]ChartPoint),
	}

	// Use channels for parallel fetching within a single stock
	type fetchResult struct {
		name string
		err  error
		data interface{}
	}

	resultChan := make(chan fetchResult, 10)
	var fetchWg sync.WaitGroup

	// Fetch quote
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		quotes, err := f.msnClient.GetQuotes([]string{id})
		if err != nil {
			resultChan <- fetchResult{name: "quote", err: err}
			return
		}
		if len(quotes) > 0 {
			resultChan <- fetchResult{name: "quote", data: &quotes[0]}
		}
	}()

	// Fetch company info
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		equities, err := f.msnClient.GetEquities([]string{id})
		if err != nil {
			resultChan <- fetchResult{name: "company", err: err}
			return
		}
		if len(equities) > 0 {
			resultChan <- fetchResult{name: "company", data: &equities[0]}
		}
	}()

	// Fetch key ratios
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		ratios, err := f.msnClient.GetKeyRatios([]string{id})
		if err != nil {
			resultChan <- fetchResult{name: "key_ratios", err: err}
			return
		}
		if len(ratios) > 0 {
			resultChan <- fetchResult{name: "key_ratios", data: &ratios[0]}
		}
	}()

	// Fetch earnings
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		earnings, err := f.msnClient.GetEarnings([]string{id})
		if err != nil {
			resultChan <- fetchResult{name: "earnings", err: err}
			return
		}
		resultChan <- fetchResult{name: "earnings", data: earnings}
	}()

	// Fetch sentiment
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		sentiment, err := f.msnClient.GetSentiment([]string{id})
		if err != nil {
			resultChan <- fetchResult{name: "sentiment", err: err}
			return
		}
		if len(sentiment) > 0 {
			resultChan <- fetchResult{name: "sentiment", data: &sentiment[0]}
		}
	}()

	// Fetch insights
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		insights, err := f.msnClient.GetInsights(id)
		if err != nil {
			resultChan <- fetchResult{name: "insights", err: err}
			return
		}
		resultChan <- fetchResult{name: "insights", data: insights}
	}()

	// Fetch financial statements
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		financials, err := f.msnClient.GetFinancialStatements(id)
		if err != nil {
			resultChan <- fetchResult{name: "financials", err: err}
			return
		}
		resultChan <- fetchResult{name: "financials", data: financials}
	}()

	// Fetch news
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		news, err := f.msnClient.GetNewsFeed(id)
		if err != nil {
			resultChan <- fetchResult{name: "news", err: err}
			return
		}
		resultChan <- fetchResult{name: "news", data: news}
	}()

	// Fetch charts (all timeframes)
	chartTypes := []string{"1D1M", "1M", "3M", "1Y", "3Y"}
	for _, chartType := range chartTypes {
		ct := chartType
		fetchWg.Add(1)
		go func() {
			defer fetchWg.Done()
			charts, err := f.msnClient.GetCharts([]string{id}, ct)
			if err != nil {
				return // Skip failed chart types silently
			}
			if len(charts) > 0 {
				points := charts[0].ToChartPoints()
				if len(points) > 0 {
					typeName := ct
					if ct == "1D1M" {
						typeName = "1D"
					}
					resultChan <- fetchResult{name: "chart_" + typeName, data: points}
				}
			}
		}()
	}

	// Fetch ownership data from Bing
	fetchWg.Add(1)
	go func() {
		defer fetchWg.Done()
		ownership, err := f.bingClient.GetAllOwnership(id, 20)
		if err != nil {
			resultChan <- fetchResult{name: "ownership", err: err}
			return
		}
		resultChan <- fetchResult{name: "ownership", data: ownership}
	}()

	// Close result channel when all fetches complete
	go func() {
		fetchWg.Wait()
		close(resultChan)
	}()

	// Collect results
	for result := range resultChan {
		if result.err != nil {
			stock.FetchStatus[result.name] = fmt.Sprintf("failed: %v", result.err)
			continue
		}

		switch result.name {
		case "quote":
			if quote, ok := result.data.(*QuoteData); ok && quote != nil {
				stock.Quote = quote
				stock.Ticker = quote.Symbol
				stock.Name = quote.ShortName
				stock.Exchange = quote.ExchangeID
				stock.FetchStatus["quote"] = "ok"
			}
		case "company":
			if equity, ok := result.data.(*EquityData); ok && equity != nil {
				stock.Company = equity
				stock.Sector = equity.Sector
				stock.Industry = equity.Industry
				if stock.Name == "" {
					stock.Name = equity.ShortName
				}
				stock.FetchStatus["company"] = "ok"
			}
		case "key_ratios":
			if ratios, ok := result.data.(*KeyRatios); ok && ratios != nil {
				stock.KeyRatios = ratios
				stock.FetchStatus["key_ratios"] = "ok"
			}
		case "earnings":
			if earnings, ok := result.data.([]EarningsEvent); ok {
				stock.Earnings = earnings
				stock.FetchStatus["earnings"] = "ok"
			}
		case "sentiment":
			if sentiment, ok := result.data.(*SentimentData); ok && sentiment != nil {
				stock.Sentiment = sentiment
				stock.FetchStatus["sentiment"] = "ok"
			}
		case "insights":
			if insights, ok := result.data.(*InsightData); ok && insights != nil {
				stock.Insights = insights
				stock.FetchStatus["insights"] = "ok"
			}
		case "financials":
			if financials, ok := result.data.(FinancialStatementsResponse); ok && len(financials) > 0 {
				stock.Financials = &FinancialData{
					Statements: financials,
				}
				stock.FetchStatus["financials"] = "ok"
			}
		case "news":
			if news, ok := result.data.([]NewsItem); ok {
				stock.News = news
				stock.FetchStatus["news"] = "ok"
			}
		case "ownership":
			if ownership, ok := result.data.(*OwnershipData); ok && ownership != nil {
				stock.Ownership = ownership
				stock.FetchStatus["ownership"] = "ok"
			}
		default:
			// Handle chart results
			if len(result.name) > 6 && result.name[:6] == "chart_" {
				chartType := result.name[6:]
				if points, ok := result.data.([]ChartPoint); ok {
					stock.Charts[chartType] = points
					stock.FetchStatus["charts"] = "ok"
				}
			}
		}
	}

	return stock
}

// FetchStockByID fetches a single stock by ID
func (f *StockFetcher) FetchStockByID(ctx context.Context, id string) (*StockData, error) {
	stocks := f.FetchStocks(ctx, []string{id}, 1)
	if len(stocks) == 0 {
		return nil, fmt.Errorf("failed to fetch stock %s", id)
	}
	return &stocks[0], nil
}
