package msn

import (
	"encoding/json"
	"fmt"

	"github.com/enetx/g"
	"github.com/enetx/surf"
)

// BingClient is the client for Bing Finance APIs (ownership data)
type BingClient struct {
	client *surf.Client
}

// NewBingClient creates a new Bing API client with Chrome impersonation
func NewBingClient() *BingClient {
	client := surf.NewClient().
		Builder().
		Impersonate().
		Chrome().
		Build().
		Unwrap()

	return &BingClient{client: client}
}

// Close closes idle connections
func (c *BingClient) Close() {
	c.client.CloseIdleConnections()
}

// commonHeaders returns common headers for Bing API requests
func (c *BingClient) commonHeaders() map[string]string {
	return map[string]string{
		"Accept":          "application/json",
		"Accept-Language": "en-US,en;q=0.9",
		"Origin":          "https://www.msn.com",
		"Referer":         "https://www.msn.com/",
	}
}

// GetTopShareHolders fetches top institutional shareholders
func (c *BingClient) GetTopShareHolders(id string, count int) ([]Holder, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}
	if count <= 0 {
		count = 50
	}

	apiURL := fmt.Sprintf("%sGetSecurityTopShareHolders/%s?rangeStart=1&count=%d",
		BingAPIBaseURL,
		id,
		count,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("top shareholders request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("top shareholders API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var result OwnershipResponse
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse top shareholders response: %w", err)
	}

	// Return whichever field has data
	if len(result.SecurityOwnerships) > 0 {
		return result.SecurityOwnerships, nil
	}
	return result.Records, nil
}

// GetTopBuyers fetches recent top buyers
func (c *BingClient) GetTopBuyers(id string, count int) ([]Holder, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}
	if count <= 0 {
		count = 50
	}

	apiURL := fmt.Sprintf("%sGetSecurityTopBuyers/%s?rangeStart=1&count=%d",
		BingAPIBaseURL,
		id,
		count,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("top buyers request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("top buyers API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var result OwnershipResponse
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse top buyers response: %w", err)
	}

	if len(result.SecurityOwnerships) > 0 {
		return result.SecurityOwnerships, nil
	}
	return result.Records, nil
}

// GetTopSellers fetches recent top sellers
func (c *BingClient) GetTopSellers(id string, count int) ([]Holder, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}
	if count <= 0 {
		count = 50
	}

	apiURL := fmt.Sprintf("%sGetSecurityTopSellers/%s?rangeStart=1&count=%d",
		BingAPIBaseURL,
		id,
		count,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("top sellers request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("top sellers API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var result OwnershipResponse
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse top sellers response: %w", err)
	}

	if len(result.SecurityOwnerships) > 0 {
		return result.SecurityOwnerships, nil
	}
	return result.Records, nil
}

// GetNewShareHolders fetches new institutional holders
func (c *BingClient) GetNewShareHolders(id string, count int) ([]Holder, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}
	if count <= 0 {
		count = 50
	}

	apiURL := fmt.Sprintf("%sGetSecurityTopNewShareHolders/%s?rangeStart=1&count=%d",
		BingAPIBaseURL,
		id,
		count,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("new shareholders request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("new shareholders API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var result OwnershipResponse
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse new shareholders response: %w", err)
	}

	if len(result.SecurityOwnerships) > 0 {
		return result.SecurityOwnerships, nil
	}
	return result.Records, nil
}

// GetExitedShareHolders fetches exited institutional holders
func (c *BingClient) GetExitedShareHolders(id string, count int) ([]Holder, error) {
	if id == "" {
		return nil, fmt.Errorf("no stock ID provided")
	}
	if count <= 0 {
		count = 50
	}

	apiURL := fmt.Sprintf("%sGetSecurityTopExitedShareHolders/%s?rangeStart=1&count=%d",
		BingAPIBaseURL,
		id,
		count,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return nil, fmt.Errorf("exited shareholders request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("exited shareholders API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var result OwnershipResponse
	if err := json.Unmarshal([]byte(body), &result); err != nil {
		return nil, fmt.Errorf("failed to parse exited shareholders response: %w", err)
	}

	if len(result.SecurityOwnerships) > 0 {
		return result.SecurityOwnerships, nil
	}
	return result.Records, nil
}

// IsInvestorDataAvailable checks if investor data exists for a stock
func (c *BingClient) IsInvestorDataAvailable(id string) (bool, error) {
	if id == "" {
		return false, fmt.Errorf("no stock ID provided")
	}

	apiURL := fmt.Sprintf("%sIsInvestorDataAvailable/%s",
		BingAPIBaseURL,
		id,
	)

	req := c.client.Get(g.String(apiURL))
	for k, v := range c.commonHeaders() {
		req = req.SetHeaders(k, v)
	}

	resp := req.Do()
	if resp.IsErr() {
		return false, fmt.Errorf("investor data check request failed: %w", resp.Err())
	}

	r := resp.Ok()
	if r.StatusCode != 200 {
		return false, fmt.Errorf("investor data check API returned status %d", r.StatusCode)
	}

	body := r.Body.String().Ok().Std()

	var available bool
	if err := json.Unmarshal([]byte(body), &available); err != nil {
		return false, fmt.Errorf("failed to parse investor data check response: %w", err)
	}

	return available, nil
}

// GetAllOwnership fetches all ownership data for a stock
func (c *BingClient) GetAllOwnership(id string, count int) (*OwnershipData, error) {
	ownership := &OwnershipData{}

	// Skip IsInvestorDataAvailable check as it often returns 404 even when data exists
	// Just try to fetch the data directly

	// Fetch all ownership data sequentially
	if holders, err := c.GetTopShareHolders(id, count); err == nil {
		ownership.TopHolders = holders
	}

	if buyers, err := c.GetTopBuyers(id, count); err == nil {
		ownership.TopBuyers = buyers
	}

	if sellers, err := c.GetTopSellers(id, count); err == nil {
		ownership.TopSellers = sellers
	}

	if newHolders, err := c.GetNewShareHolders(id, count); err == nil {
		ownership.NewHolders = newHolders
	}

	if exited, err := c.GetExitedShareHolders(id, count); err == nil {
		ownership.ExitedHolders = exited
	}

	return ownership, nil
}
