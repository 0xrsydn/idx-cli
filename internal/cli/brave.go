package cli

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"time"

	"github.com/enetx/g"
	"github.com/enetx/surf"
)

// BraveResult represents a single news result from Brave Search API
type BraveResult struct {
	Title       string `json:"title"`
	URL         string `json:"url"`
	Description string `json:"description"`
	PageAge     string `json:"page_age"`
}

// BraveNewsResponse represents the Brave News Search API response
type BraveNewsResponse struct {
	Results []struct {
		Title       string `json:"title"`
		URL         string `json:"url"`
		Description string `json:"description"`
		Age         string `json:"age"`
	} `json:"results"`
}

// SearchConfig holds search parameters
type SearchConfig struct {
	Query string
	From  time.Time
	To    time.Time
	Count int
}

// SearchBrave queries the Brave News Search API
// Uses dedicated News endpoint: GET /res/v1/news/search
func SearchBrave(client *surf.Client, config SearchConfig) ([]BraveResult, error) {
	apiKey := os.Getenv("BRAVE_API_KEY")
	if apiKey == "" {
		return nil, fmt.Errorf("BRAVE_API_KEY environment variable not set")
	}

	// Build query parameters
	params := url.Values{}
	params.Set("q", config.Query)
	params.Set("count", fmt.Sprintf("%d", config.Count))
	params.Set("freshness", fmt.Sprintf("%sto%s",
		config.From.Format("2006-01-02"),
		config.To.Format("2006-01-02"),
	))

	apiURL := fmt.Sprintf("https://api.search.brave.com/res/v1/news/search?%s", params.Encode())

	// Use plain client for API calls (no Chrome impersonation which overrides headers)
	apiClient := surf.NewClient()
	defer apiClient.CloseIdleConnections()

	resp := apiClient.Get(g.String(apiURL)).
		SetHeaders("Accept", "application/json").
		SetHeaders("X-Subscription-Token", apiKey).
		// Jakarta, Indonesia location headers
		SetHeaders("X-Loc-Lat", "-6.2088").
		SetHeaders("X-Loc-Long", "106.8456").
		SetHeaders("X-Loc-Timezone", "Asia/Jakarta").
		SetHeaders("X-Loc-Country", "ID").
		Do()

	if resp.IsErr() {
		return nil, fmt.Errorf("brave API request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		body := r.Body.String().Ok().Std()
		return nil, fmt.Errorf("brave API returned status %d: %s", r.StatusCode, body)
	}

	body := r.Body.String().Ok().Std()

	var braveResp BraveNewsResponse
	if err := json.Unmarshal([]byte(body), &braveResp); err != nil {
		return nil, fmt.Errorf("failed to parse brave API response: %w", err)
	}

	if len(braveResp.Results) == 0 {
		return []BraveResult{}, nil
	}

	results := make([]BraveResult, len(braveResp.Results))
	for i, r := range braveResp.Results {
		results[i] = BraveResult{
			Title:       r.Title,
			URL:         r.URL,
			Description: r.Description,
			PageAge:     r.Age,
		}
	}

	return results, nil
}

// BuildStockQuery creates a boolean query for Indonesian stock news
// Example: BuildStockQuery("MINA", "MINA Tbk") returns:
// ("MINA" OR "MINA Tbk") AND (saham OR emiten OR "Bursa Efek Indonesia" OR BEI OR IDX)
func BuildStockQuery(stockTerms ...string) string {
	if len(stockTerms) == 0 {
		return ""
	}

	// Build stock terms part
	stockPart := "("
	for i, term := range stockTerms {
		if i > 0 {
			stockPart += " OR "
		}
		stockPart += fmt.Sprintf(`"%s"`, term)
	}
	stockPart += ")"

	// Indonesian stock market keywords
	marketKeywords := `(saham OR emiten OR "Bursa Efek Indonesia" OR BEI OR IDX)`

	return stockPart + " AND " + marketKeywords
}
