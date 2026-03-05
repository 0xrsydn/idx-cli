package msn

import (
	"encoding/json"
	"fmt"
	"net/url"
	"strings"
	"time"

	"github.com/enetx/g"
	"github.com/enetx/surf"
)

// MSNClientConfig holds configuration for the MSN client
type MSNClientConfig struct {
	Proxy       string // Proxy URL (http://, https://, socks5://)
	RateLimiter *RateLimiter
}

// MSNClient is the base client for MSN Finance APIs
type MSNClient struct {
	client      *surf.Client
	proxy       string
	rateLimiter *RateLimiter
}

// NewMSNClient creates a new MSN API client with Chrome impersonation
func NewMSNClient() *MSNClient {
	return NewMSNClientWithConfig(MSNClientConfig{})
}

// NewMSNClientWithConfig creates a new MSN API client with custom configuration
func NewMSNClientWithConfig(config MSNClientConfig) *MSNClient {
	builder := surf.NewClient().
		Builder().
		Impersonate().
		Chrome()

	// Add proxy if configured
	if config.Proxy != "" {
		builder = builder.Proxy(g.String(config.Proxy))
	}

	client := builder.Build().Unwrap()

	return &MSNClient{
		client:      client,
		proxy:       config.Proxy,
		rateLimiter: config.RateLimiter,
	}
}

// waitForRateLimit waits for rate limiter if configured
func (c *MSNClient) waitForRateLimit() {
	if c.rateLimiter != nil {
		c.rateLimiter.Wait()
	}
}

// Close closes idle connections
func (c *MSNClient) Close() {
	c.client.CloseIdleConnections()
}

// commonHeaders returns common headers for MSN API requests
func (c *MSNClient) commonHeaders() map[string]string {
	return map[string]string{
		"Accept":          "application/json",
		"Accept-Language": "en-US,en;q=0.9,id;q=0.8",
		"Origin":          "https://www.msn.com",
		"Referer":         "https://www.msn.com/",
	}
}

// GetQuotes fetches real-time quotes for given stock IDs
func (c *MSNClient) GetQuotes(ids []string) ([]QuoteData, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("no stock IDs provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sFinance/Quotes?apikey=%s&ids=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		strings.Join(ids, ","),
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("quotes request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("quotes API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var quotes []QuoteData
	if err := json.Unmarshal([]byte(body), &quotes); err != nil {
		return nil, fmt.Errorf("failed to parse quotes response: %w", err)
	}

	return quotes, nil
}

// GetQuoteSummary fetches detailed quote summary with multiple intents
func (c *MSNClient) GetQuoteSummary(id string, intents []string) (map[string]json.RawMessage, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}

	c.waitForRateLimit()

	intentStr := strings.Join(intents, ",")
	apiURL := fmt.Sprintf("%sFinance/QuoteSummary?apikey=%s&ids=%s&intents=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		id,
		intentStr,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("quote summary request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("quote summary API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var result []map[string]json.RawMessage
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse quote summary response: %w", err)
	}

	if len(result) == 0 {
		return nil, fmt.Errorf("empty quote summary response")
	}

	return result[0], nil
}

// GetCharts fetches historical chart data
func (c *MSNClient) GetCharts(ids []string, chartType string) ([]ChartResponse, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("no stock IDs provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sFinance/Charts?apikey=%s&cm=id-id&ids=%s&type=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		strings.Join(ids, ","),
		chartType,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("charts request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("charts API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var charts []ChartResponse
	if err := json.Unmarshal([]byte(body), &charts); err != nil {
		return nil, fmt.Errorf("failed to parse charts response: %w", err)
	}

	return charts, nil
}

// GetEquities fetches company information
func (c *MSNClient) GetEquities(ids []string) ([]EquityData, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("no stock IDs provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sFinance/Equities?apikey=%s&ids=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		strings.Join(ids, ","),
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("equities request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("equities API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var equities []EquityData
	if err := json.Unmarshal([]byte(body), &equities); err != nil {
		return nil, fmt.Errorf("failed to parse equities response: %w", err)
	}

	return equities, nil
}

// GetFinancialStatements fetches financial statements
func (c *MSNClient) GetFinancialStatements(id string) (FinancialStatementsResponse, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}

	c.waitForRateLimit()

	// URL encode the filter parameter
	filter := fmt.Sprintf("_p eq '%s'", id)
	apiURL := fmt.Sprintf("%sFinance/Equities/financialstatements?apikey=%s&$filter=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		url.QueryEscape(filter),
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("financial statements request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("financial statements API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	// Response is a direct array of FinancialStatement
	var result FinancialStatementsResponse
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse financial statements response: %w", err)
	}

	return result, nil
}

// GetEarnings fetches earnings events
func (c *MSNClient) GetEarnings(ids []string) ([]EarningsEvent, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("no stock IDs provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sFinance/Events/Earnings?apikey=%s&ids=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		strings.Join(ids, ","),
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("earnings request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("earnings API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	// Parse the actual API response format
	var apiResp EarningsAPIResponse
	if err := json.Unmarshal([]byte(body), &apiResp); err != nil {
		return nil, fmt.Errorf("failed to parse earnings response: %w", err)
	}

	// Convert quarterly earnings to EarningsEvent array
	var earnings []EarningsEvent
	for periodKey, data := range apiResp.History.Quarterly {
		// Parse fiscal year and quarter from CiqFiscalPeriodType (e.g., "Q42025")
		fiscalYear := 0
		fiscalQuarter := 0
		if len(data.CiqFiscalPeriodType) >= 6 {
			// Format: Q{quarter}{year} e.g., Q42025
			fmt.Sscanf(data.CiqFiscalPeriodType, "Q%d%d", &fiscalQuarter, &fiscalYear)
		}
		if fiscalYear == 0 && len(periodKey) >= 6 {
			// Fallback: parse from period key (e.g., "202512")
			fmt.Sscanf(periodKey[:4], "%d", &fiscalYear)
			month := 0
			fmt.Sscanf(periodKey[4:6], "%d", &month)
			fiscalQuarter = (month-1)/3 + 1
		}

		// Parse event date
		eventDate := ""
		if data.EarningReleaseDate != "" {
			// Extract date portion from ISO timestamp
			if len(data.EarningReleaseDate) >= 10 {
				eventDate = data.EarningReleaseDate[:10]
			}
		}

		earnings = append(earnings, EarningsEvent{
			ID:              fmt.Sprintf("%s_%s", apiResp.InstrumentID, periodKey),
			EventDate:       eventDate,
			FiscalYear:      fiscalYear,
			FiscalQuarter:   fiscalQuarter,
			EPSEstimate:     data.EpsForecast,
			EPSActual:       data.EpsActual,
			EPSSurprise:     data.EpsSurprise,
			EPSSurprisePct:  data.EpsSurprisePercent,
			RevenueEstimate: data.RevenueForecast,
			RevenueActual:   data.RevenueActual,
			RevenueSurprise: data.RevenueSurprise,
		})
	}

	return earnings, nil
}

// GetSentiment fetches market sentiment
func (c *MSNClient) GetSentiment(ids []string) ([]SentimentData, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("no stock IDs provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sFinance/SentimentBrowser?apikey=%s&cm=id-id&it=web&scn=ANON&ids=%s&wrapodata=false&flightId=INeedDau",
		MSNAssetsBaseURL,
		MSNAPIKey,
		strings.Join(ids, ","),
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("sentiment request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("sentiment API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var sentiment []SentimentData
	if err := json.Unmarshal([]byte(body), &sentiment); err != nil {
		return nil, fmt.Errorf("failed to parse sentiment response: %w", err)
	}

	return sentiment, nil
}

// GetKeyRatios fetches key financial ratios from api.msn.com
func (c *MSNClient) GetKeyRatios(ids []string) ([]KeyRatios, error) {
	if len(ids) == 0 {
		return nil, fmt.Errorf("no stock IDs provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%skeyratios?apikey=%s&ids=%s&wrapodata=false",
		MSNAPIBaseURL,
		MSNAPIKey,
		strings.Join(ids, ","),
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("key ratios request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("key ratios API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var ratios []KeyRatios
	if err := json.Unmarshal([]byte(body), &ratios); err != nil {
		return nil, fmt.Errorf("failed to parse key ratios response: %w", err)
	}

	return ratios, nil
}

// GetInsights fetches AI-generated insights from api.msn.com
func (c *MSNClient) GetInsights(id string) (*InsightData, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sinsights?apikey=%s&ids=%s&wrapodata=false",
		MSNAPIBaseURL,
		MSNAPIKey,
		id,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("insights request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("insights API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var insights []InsightData
	if err := json.Unmarshal([]byte(body), &insights); err != nil {
		return nil, fmt.Errorf("failed to parse insights response: %w", err)
	}

	if len(insights) == 0 {
		return nil, nil
	}

	return &insights[0], nil
}

// GetNewsFeed fetches stock-related news
func (c *MSNClient) GetNewsFeed(id string) ([]NewsItem, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}

	c.waitForRateLimit()

	// Use the stock-specific entity feed format from MSN website
	apiURL := fmt.Sprintf("%sMSN/Feed/me?$top=30&apikey=%s&cm=id-id&contentType=article,video,slideshow&it=web&query=ef_stock_%s&queryType=entityfeed&responseSchema=cardview&scn=ANON&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
		id,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("news feed request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("news feed API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var newsFeed NewsFeedResponse
	if err := json.Unmarshal([]byte(body), &newsFeed); err != nil {
		return nil, fmt.Errorf("failed to parse news feed response: %w", err)
	}

	// Use SubCards if available (cardview response), otherwise use Value
	if len(newsFeed.SubCards) > 0 {
		return newsFeed.SubCards, nil
	}
	return newsFeed.Value, nil
}

// GetAllCharts fetches all chart timeframes for a stock
func (c *MSNClient) GetAllCharts(id string) (map[string][]ChartPoint, error) {
	chartTypes := []string{"1D1M", "1M", "3M", "1Y", "3Y"}
	result := make(map[string][]ChartPoint)

	for _, chartType := range chartTypes {
		charts, err := c.GetCharts([]string{id}, chartType)
		if err != nil {
			continue // Skip failed chart types
		}
		if len(charts) > 0 {
			// Map chart type to friendlier names
			typeName := chartType
			switch chartType {
			case "1D1M":
				typeName = "1D"
			}
			result[typeName] = charts[0].Points
		}
	}

	return result, nil
}

// FetchStockData fetches all data for a single stock
func (c *MSNClient) FetchStockData(id string) (*StockData, error) {
	stock := &StockData{
		ID:          id,
		FetchedAt:   time.Now().UTC().Format(time.RFC3339),
		FetchStatus: make(map[string]string),
		Charts:      make(map[string][]ChartPoint),
	}

	// Fetch quote
	quotes, err := c.GetQuotes([]string{id})
	if err != nil {
		stock.FetchStatus["quote"] = fmt.Sprintf("failed: %v", err)
	} else if len(quotes) > 0 {
		stock.Quote = &quotes[0]
		stock.Ticker = quotes[0].Symbol
		stock.Name = quotes[0].ShortName
		stock.Exchange = quotes[0].ExchangeID
		stock.FetchStatus["quote"] = "ok"
	}

	// Fetch company info
	equities, err := c.GetEquities([]string{id})
	if err != nil {
		stock.FetchStatus["company"] = fmt.Sprintf("failed: %v", err)
	} else if len(equities) > 0 {
		stock.Company = &equities[0]
		stock.Sector = equities[0].Sector
		stock.Industry = equities[0].Industry
		if stock.Name == "" {
			stock.Name = equities[0].ShortName
		}
		stock.FetchStatus["company"] = "ok"
	}

	// Fetch charts
	charts, err := c.GetAllCharts(id)
	if err != nil {
		stock.FetchStatus["charts"] = fmt.Sprintf("failed: %v", err)
	} else {
		stock.Charts = charts
		stock.FetchStatus["charts"] = "ok"
	}

	// Fetch key ratios
	ratios, err := c.GetKeyRatios([]string{id})
	if err != nil {
		stock.FetchStatus["key_ratios"] = fmt.Sprintf("failed: %v", err)
	} else if len(ratios) > 0 {
		stock.KeyRatios = &ratios[0]
		stock.FetchStatus["key_ratios"] = "ok"
	}

	// Fetch earnings
	earnings, err := c.GetEarnings([]string{id})
	if err != nil {
		stock.FetchStatus["earnings"] = fmt.Sprintf("failed: %v", err)
	} else {
		stock.Earnings = earnings
		stock.FetchStatus["earnings"] = "ok"
	}

	// Fetch sentiment
	sentiment, err := c.GetSentiment([]string{id})
	if err != nil {
		stock.FetchStatus["sentiment"] = fmt.Sprintf("failed: %v", err)
	} else if len(sentiment) > 0 {
		stock.Sentiment = &sentiment[0]
		stock.FetchStatus["sentiment"] = "ok"
	}

	// Fetch insights
	insights, err := c.GetInsights(id)
	if err != nil {
		stock.FetchStatus["insights"] = fmt.Sprintf("failed: %v", err)
	} else if insights != nil {
		stock.Insights = insights
		stock.FetchStatus["insights"] = "ok"
	}

	// Fetch financial statements
	financials, err := c.GetFinancialStatements(id)
	if err != nil {
		stock.FetchStatus["financials"] = fmt.Sprintf("failed: %v", err)
	} else if len(financials) > 0 {
		stock.Financials = &FinancialData{
			Statements: financials,
		}
		stock.FetchStatus["financials"] = "ok"
	}

	// Fetch news
	news, err := c.GetNewsFeed(id)
	if err != nil {
		stock.FetchStatus["news"] = fmt.Sprintf("failed: %v", err)
	} else {
		stock.News = news
		stock.FetchStatus["news"] = "ok"
	}

	return stock, nil
}
