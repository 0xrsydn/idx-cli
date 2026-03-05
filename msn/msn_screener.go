package msn

import (
	"encoding/json"
	"fmt"

	"github.com/enetx/g"
)

// ScreenerFilter represents available screener filter presets
type ScreenerFilter string

const (
	FilterTopPerformers   ScreenerFilter = "top-performers"
	FilterWorstPerformers ScreenerFilter = "worst-performers"
	FilterHighDividend    ScreenerFilter = "high-dividend"
	FilterLowPE           ScreenerFilter = "low-pe"
	Filter52WeekHigh      ScreenerFilter = "52w-high"
	Filter52WeekLow       ScreenerFilter = "52w-low"
	FilterHighVolume      ScreenerFilter = "high-volume"
	FilterLargeMarketCap  ScreenerFilter = "large-cap"
)

// Filter key mappings for MSN Screener API
var screenerFilterKeys = map[ScreenerFilter]string{
	FilterTopPerformers:   "st_list_topperfs",
	FilterWorstPerformers: "st_list_poorperfs",
	FilterHighDividend:    "st_list_highdividend",
	FilterLowPE:           "st_list_lowpe",
	Filter52WeekHigh:      "st_list_52wkhi",
	Filter52WeekLow:       "st_list_52wklow",
	FilterHighVolume:      "st_list_highvol",
	FilterLargeMarketCap:  "st_list_largecap",
}

// Region key mappings for MSN Screener API
var screenerRegionKeys = map[string]string{
	"id": "st_reg_id", // Indonesia
	"us": "st_reg_us", // United States
	"gb": "st_reg_gb", // United Kingdom
	"de": "st_reg_de", // Germany
	"jp": "st_reg_jp", // Japan
	"hk": "st_reg_hk", // Hong Kong
	"sg": "st_reg_sg", // Singapore
	"au": "st_reg_au", // Australia
	"in": "st_reg_in", // India
	"cn": "st_reg_cn", // China
}

// ScreenerConfig holds screener configuration
type ScreenerConfig struct {
	Region    string         // Country code (e.g., "id" for Indonesia)
	Filter    ScreenerFilter // Filter preset
	Limit     int            // Max results
	PageIndex int            // Page number (0-indexed)
}

// ScreenerAPIResponse is the raw response from Finance/Screener
type ScreenerAPIResponse struct {
	Count    int           `json:"count"`
	MatchIDs []string      `json:"matchIds"`
	Quote    []QuoteData   `json:"quote"`
	Equity   []EquityData  `json:"equity"`
	Fund     []interface{} `json:"fund"`
}

// RunScreener executes the stock screener with given configuration
func (c *MSNClient) RunScreener(config ScreenerConfig) (*ScreenerResponse, error) {
	if config.Region == "" {
		config.Region = "id" // Default to Indonesia
	}
	if config.Limit <= 0 {
		config.Limit = 50
	}

	// Build filter array
	filters := buildScreenerFilters(config.Region, config.Filter)

	req := ScreenerRequest{
		Filter:          filters,
		Order:           ScreenerOrder{Key: "st_1yr_asc_order", Dir: "desc"},
		ReturnValueType: []string{"quote", "equity"},
		ScreenerType:    "stock",
		Limit:           config.Limit,
	}

	reqBody, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal screener request: %w", err)
	}

	c.waitForRateLimit()

	apiURL := fmt.Sprintf("%sFinance/Screener?apikey=%s&wrapodata=false",
		MSNAssetsBaseURL,
		MSNAPIKey,
	)

	httpReq := c.client.Post(g.String(apiURL)).
		SetHeaders("Content-Type", "text/plain;charset=UTF-8")
	for k, v := range c.commonHeaders() {
		httpReq = httpReq.SetHeaders(k, v)
	}
	httpReq = httpReq.Body(g.String(string(reqBody)))

	resp := httpReq.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("screener request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		body := r.Body.String().Ok().Std()
		return nil, fmt.Errorf("screener API returned status %d: %s", r.StatusCode, body)
	}

	body := r.Body.String().Ok().Std()

	var apiResp ScreenerAPIResponse
	if err := json.Unmarshal([]byte(body), &apiResp); err != nil {
		return nil, fmt.Errorf("failed to parse screener response: %w", err)
	}

	// Merge quote and equity data into ScreenerStock
	stocks := mergeScreenerResults(apiResp)

	return &ScreenerResponse{
		Value:    stocks,
		Total:    apiResp.Count,
		Count:    apiResp.Count,
		MatchIDs: apiResp.MatchIDs,
	}, nil
}

// buildScreenerFilters creates filter array based on region and preset
func buildScreenerFilters(region string, filter ScreenerFilter) []ScreenerFilterItem {
	filters := make([]ScreenerFilterItem, 0, 2)

	// Add filter preset
	if filterKey, ok := screenerFilterKeys[filter]; ok {
		filters = append(filters, ScreenerFilterItem{
			Key:      filterKey,
			KeyGroup: "st_list_",
			IsRange:  false,
		})
	}

	// Add region filter
	if regionKey, ok := screenerRegionKeys[region]; ok {
		filters = append(filters, ScreenerFilterItem{
			Key:      regionKey,
			KeyGroup: "st_reg_",
			IsRange:  false,
		})
	}

	return filters
}

// mergeScreenerResults combines quote and equity data into ScreenerStock slice
func mergeScreenerResults(apiResp ScreenerAPIResponse) []ScreenerStock {
	// Build equity map by instrumentId
	equityMap := make(map[string]*EquityData)
	for i := range apiResp.Equity {
		eq := &apiResp.Equity[i]
		// Use instrumentId from the "_p" field if available
		if id := eq.ID; id != "" {
			equityMap[id] = eq
		}
	}

	stocks := make([]ScreenerStock, 0, len(apiResp.Quote))
	for _, q := range apiResp.Quote {
		stock := ScreenerStock{
			ID:             q.InstrumentID,
			InstrumentID:   q.InstrumentID,
			Symbol:         q.Symbol,
			ShortName:      q.ShortName,
			DisplayName:    q.DisplayName,
			ExchangeID:     q.ExchangeID,
			ExchangeCode:   q.ExchangeCode,
			Country:        q.Country,
			Price:          q.Price,
			PriceChange:    q.PriceChange,
			PriceChangePct: q.PriceChangePct,
			MarketCap:      q.MarketCap,
			Volume:         q.AccumulatedVolume,
			Price52wHigh:   q.Price52wHigh,
			Price52wLow:    q.Price52wLow,
			Return1Year:    q.Return1Year,
			ReturnYTD:      q.ReturnYTD,
		}

		// Merge equity data if available
		if eq, ok := equityMap[q.InstrumentID]; ok {
			stock.Sector = eq.Sector
			stock.Industry = eq.Industry
		}

		stocks = append(stocks, stock)
	}

	return stocks
}

// ParseScreenerFilter converts string to ScreenerFilter
func ParseScreenerFilter(s string) (ScreenerFilter, error) {
	switch s {
	case "top-performers", "top":
		return FilterTopPerformers, nil
	case "worst-performers", "worst":
		return FilterWorstPerformers, nil
	case "high-dividend", "dividend":
		return FilterHighDividend, nil
	case "low-pe", "pe":
		return FilterLowPE, nil
	case "52w-high", "52high":
		return Filter52WeekHigh, nil
	case "52w-low", "52low":
		return Filter52WeekLow, nil
	case "high-volume", "volume":
		return FilterHighVolume, nil
	case "large-cap", "largecap":
		return FilterLargeMarketCap, nil
	default:
		return "", fmt.Errorf("unknown filter: %s (valid: top-performers, worst-performers, high-dividend, low-pe, 52w-high, 52w-low, high-volume, large-cap)", s)
	}
}
