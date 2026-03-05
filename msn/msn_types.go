package msn

// MSN API Constants
const (
	MSNAssetsBaseURL = "https://assets.msn.com/service/"
	MSNAPIBaseURL    = "https://api.msn.com/msn/v0/pages/finance/"
	BingAPIBaseURL   = "https://services.bingapis.com/contentservices-finance.hedgefunddataprovider/api/v1/"

	// Public API key from MSN Money website
	MSNAPIKey = "0QfOX3Vn51YCzitbLaRkTTBadtWpgTN8NZLW0C1SEM"
)

// ScreenerRequest is the POST body for Finance/Screener
// Uses the actual MSN API format with predefined filter keys
type ScreenerRequest struct {
	Filter          []ScreenerFilterItem `json:"filter"`
	Order           ScreenerOrder        `json:"order"`
	ReturnValueType []string             `json:"returnValueType"`
	ScreenerType    string               `json:"screenerType"`
	Limit           int                  `json:"limit"`
}

// ScreenerFilterItem represents a filter condition in the screener
type ScreenerFilterItem struct {
	Key      string `json:"key"`      // e.g., "st_list_topperfs", "st_reg_id"
	KeyGroup string `json:"keyGroup"` // e.g., "st_list_", "st_reg_"
	IsRange  bool   `json:"isRange"`
}

// ScreenerOrder represents sort order for screener results
type ScreenerOrder struct {
	Key string `json:"key"` // e.g., "st_1yr_asc_order"
	Dir string `json:"dir"` // "asc" or "desc"
}

// ScreenerResponse from Finance/Screener
type ScreenerResponse struct {
	Value    []ScreenerStock `json:"value"`
	Total    int             `json:"total"`
	Count    int             `json:"count"`
	MatchIDs []string        `json:"matchIds"`
	Equity   []ScreenerStock `json:"equity"`
	Quote    []QuoteData     `json:"quote"`
}

// ScreenerStock is a stock from screener results
type ScreenerStock struct {
	ID             string  `json:"id"`
	InstrumentID   string  `json:"instrumentId,omitempty"`
	Symbol         string  `json:"symbol"`
	ShortName      string  `json:"shortName"`
	DisplayName    string  `json:"displayName,omitempty"`
	ExchangeID     string  `json:"exchangeId"`
	ExchangeCode   string  `json:"exchangeCode,omitempty"`
	Country        string  `json:"country,omitempty"`
	Sector         string  `json:"sector,omitempty"`
	Industry       string  `json:"industry,omitempty"`
	Price          float64 `json:"price"`
	PriceChange    float64 `json:"priceChange"`
	PriceChangePct float64 `json:"priceChangePercent"`
	MarketCap      float64 `json:"marketCap"`
	Volume         float64 `json:"accumulatedVolume"`
	Price52wHigh   float64 `json:"price52wHigh"`
	Price52wLow    float64 `json:"price52wLow"`
	Return1Year    float64 `json:"return1Year"`
	ReturnYTD      float64 `json:"returnYTD"`
}

// QuoteResponse from Finance/Quotes
type QuoteResponse []QuoteData

// QuoteData represents real-time quote data
type QuoteData struct {
	ID                 string  `json:"id"`
	InstrumentID       string  `json:"instrumentId"`
	Symbol             string  `json:"symbol"`
	ShortName          string  `json:"shortName"`
	DisplayName        string  `json:"displayName"`
	Price              float64 `json:"price"`
	PriceChange        float64 `json:"priceChange"`
	PriceChangePct     float64 `json:"priceChangePercent"`
	PriceDayOpen       float64 `json:"priceDayOpen"`
	PriceDayHigh       float64 `json:"priceDayHigh"`
	PriceDayLow        float64 `json:"priceDayLow"`
	PricePreviousClose float64 `json:"pricePreviousClose"`
	PriceClose         float64 `json:"priceClose"`
	Price52wHigh       float64 `json:"price52wHigh"`
	Price52wLow        float64 `json:"price52wLow"`
	AccumulatedVolume  float64 `json:"accumulatedVolume"`
	AverageVolume      float64 `json:"averageVolume"`
	MarketCap          float64 `json:"marketCap"`
	MarketCapCurrency  string  `json:"marketCapCurrency"`
	ExchangeID         string  `json:"exchangeId"`
	ExchangeCode       string  `json:"exchangeCode"`
	ExchangeName       string  `json:"exchangeName"`
	Currency           string  `json:"currency"`
	Country            string  `json:"country"`
	Market             string  `json:"market"`
	TimeLastTraded     string  `json:"timeLastTraded"`
	TimeLastUpdated    string  `json:"timeLastUpdated"`
	// Historical price changes
	PriceChange1Week  float64 `json:"priceChange1Week"`
	PriceChange1Month float64 `json:"priceChange1Month"`
	PriceChange3Month float64 `json:"priceChange3Month"`
	PriceChange6Month float64 `json:"priceChange6Month"`
	PriceChangeYTD    float64 `json:"priceChangeYTD"`
	PriceChange1Year  float64 `json:"priceChange1Year"`
	// Historical returns (percentage)
	Return1Week  float64 `json:"return1Week"`
	Return1Month float64 `json:"return1Month"`
	Return3Month float64 `json:"return3Month"`
	Return6Month float64 `json:"return6Month"`
	ReturnYTD    float64 `json:"returnYTD"`
	Return1Year  float64 `json:"return1Year"`
}

// QuoteSummaryResponse from Finance/QuoteSummary
type QuoteSummaryResponse []struct {
	Quotes    []QuoteData     `json:"quotes"`
	Exchanges []ExchangeData  `json:"exchanges"`
	Details   []QuoteDetail   `json:"quoteDetails"`
	ChartData []ChartResponse `json:"charts"`
}

// ExchangeData from Finance/Exchanges
type ExchangeData struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	Country  string `json:"country"`
	Timezone string `json:"timeZone"`
}

// QuoteDetail provides extended quote information
type QuoteDetail struct {
	ID              string  `json:"id"`
	Beta            float64 `json:"beta"`
	TrailingPE      float64 `json:"trailingPE"`
	ForwardPE       float64 `json:"forwardPE"`
	PriceToBook     float64 `json:"priceToBook"`
	PriceToSales    float64 `json:"priceToSales"`
	EnterpriseValue float64 `json:"enterpriseValue"`
	EBITDA          float64 `json:"ebitda"`
	Revenue         float64 `json:"revenue"`
	GrossProfit     float64 `json:"grossProfit"`
	FreeCashFlow    float64 `json:"freeCashFlow"`
	DebtToEquity    float64 `json:"debtToEquity"`
	QuickRatio      float64 `json:"quickRatio"`
	CurrentRatio    float64 `json:"currentRatio"`
	ReturnOnEquity  float64 `json:"returnOnEquity"`
	ReturnOnAssets  float64 `json:"returnOnAssets"`
	ProfitMargin    float64 `json:"profitMargin"`
	OperatingMargin float64 `json:"operatingMargin"`
	GrossMargin     float64 `json:"grossMargin"`
}

// ChartResponse from Finance/Charts
type ChartResponse struct {
	ID        string          `json:"_p"`
	ChartType string          `json:"chartType"` // "1D1M", "1M", "3M", "1Y", "3Y"
	Symbol    string          `json:"symbol"`
	Series    ChartSeriesData `json:"series"`
	Points    []ChartPoint    `json:"-"` // Computed from Series
}

// ChartSeriesData is the raw series data from the API
type ChartSeriesData struct {
	TimeStamps []string  `json:"timeStamps"`
	Prices     []float64 `json:"prices"`
	OpenPrices []float64 `json:"openPrices"`
	PricesHigh []float64 `json:"pricesHigh"`
	PricesLow  []float64 `json:"pricesLow"`
	Volumes    []float64 `json:"volumes"`
	StartTime  string    `json:"startTime"`
	EndTime    string    `json:"endTime"`
	PriceHigh  float64   `json:"priceHigh"`
	PriceLow   float64   `json:"priceLow"`
}

// ToChartPoints converts the series data into chart points
func (c *ChartResponse) ToChartPoints() []ChartPoint {
	if len(c.Series.TimeStamps) == 0 {
		return nil
	}

	points := make([]ChartPoint, len(c.Series.TimeStamps))
	for i, ts := range c.Series.TimeStamps {
		point := ChartPoint{Time: ts}

		if i < len(c.Series.Prices) {
			point.Price = c.Series.Prices[i]
			point.Close = c.Series.Prices[i]
		}
		if i < len(c.Series.OpenPrices) {
			point.Open = c.Series.OpenPrices[i]
		}
		if i < len(c.Series.PricesHigh) {
			point.High = c.Series.PricesHigh[i]
		}
		if i < len(c.Series.PricesLow) {
			point.Low = c.Series.PricesLow[i]
		}
		if i < len(c.Series.Volumes) {
			point.Volume = int64(c.Series.Volumes[i])
		}
		points[i] = point
	}
	return points
}

// ChartPoint is a single data point in a chart
type ChartPoint struct {
	Time   string  `json:"time"`
	Price  float64 `json:"price"`
	Open   float64 `json:"open"`
	High   float64 `json:"high"`
	Low    float64 `json:"low"`
	Close  float64 `json:"close"`
	Volume int64   `json:"volume"`
}

// EquityResponse from Finance/Equities
type EquityResponse []EquityData

// EquityData represents company information
type EquityData struct {
	ID          string    `json:"id"`
	Symbol      string    `json:"symbol"`
	ShortName   string    `json:"shortName"`
	LongName    string    `json:"longName"`
	Description string    `json:"description"`
	Sector      string    `json:"sector"`
	Industry    string    `json:"industry"`
	Website     string    `json:"website"`
	Employees   int       `json:"fullTimeEmployees"`
	Address     string    `json:"address"`
	City        string    `json:"city"`
	Country     string    `json:"country"`
	Phone       string    `json:"phone"`
	Officers    []Officer `json:"officers"`
}

// Officer represents a company executive
type Officer struct {
	Name     string `json:"name"`
	Title    string `json:"title"`
	Age      int    `json:"age"`
	YearBorn int    `json:"yearBorn"`
	TotalPay int64  `json:"totalPay"`
}

// FinancialStatementsResponse from Finance/Equities/financialstatements
// Response is an array of FinancialStatement objects
type FinancialStatementsResponse []FinancialStatement

// FinancialStatement represents comprehensive financial data
type FinancialStatement struct {
	UnderlyingInstrument InstrumentInfo   `json:"underlyingInstrument"`
	BalanceSheets        *BalanceSheet    `json:"balanceSheets"`
	CashFlow             *CashFlowData    `json:"cashFlow"`
	IncomeStatements     *IncomeStatement `json:"incomeStatements"`
}

// InstrumentInfo contains basic stock information
type InstrumentInfo struct {
	InstrumentID string `json:"instrumentId"`
	DisplayName  string `json:"displayName"`
	ShortName    string `json:"shortName"`
	ExchangeID   string `json:"exchangeId"`
	ExchangeCode string `json:"exchangeCode"`
	SecurityType string `json:"securityType"`
	Symbol       string `json:"symbol"`
}

// BalanceSheet represents balance sheet data
type BalanceSheet struct {
	CurrentAssets      map[string]float64 `json:"currentAssets"`
	LongTermAssets     map[string]float64 `json:"longTermAssets"`
	CurrentLiabilities map[string]float64 `json:"currentLiabilities"`
	Equity             map[string]float64 `json:"equity"`
	Currency           string             `json:"currency"`
	Source             string             `json:"source"`
	SourceDate         string             `json:"sourceDate"`
	ReportDate         string             `json:"reportDate"`
	EndDate            string             `json:"endDate"`
}

// CashFlowData represents cash flow statement
type CashFlowData struct {
	Financing map[string]float64 `json:"financing"`
	Investing map[string]float64 `json:"investing"`
	Operating map[string]float64 `json:"operating"`
	Currency  string             `json:"currency"`
	Source    string             `json:"source"`
	EndDate   string             `json:"endDate"`
}

// IncomeStatement represents income statement data
type IncomeStatement struct {
	Revenue  map[string]float64 `json:"revenue"`
	Expenses map[string]float64 `json:"expenses"`
	Currency string             `json:"currency"`
	Source   string             `json:"source"`
	EndDate  string             `json:"endDate"`
}

// KeyRatiosResponse from api.msn.com keyratios
type KeyRatiosResponse []KeyRatios

// KeyRatios represents financial ratios with historical data
type KeyRatios struct {
	StockID         string           `json:"stockId"`
	ExchangeID      string           `json:"exchangeId"`
	Market          string           `json:"market"`
	Industry        string           `json:"industry"`
	DisplayName     string           `json:"displayName"`
	ShortName       string           `json:"shortName"`
	Symbol          string           `json:"symbol"`
	IndustryMetrics []IndustryMetric `json:"industryMetrics"`
}

// IndustryMetric represents financial metrics for a specific year
type IndustryMetric struct {
	Year                 string  `json:"year"`
	FiscalPeriodType     string  `json:"fiscalPeriodType"`
	RevenuePerShare      float64 `json:"revenuePerShare"`
	EarningsPerShare     float64 `json:"earningsPerShare"`
	FreeCashFlowPerShare float64 `json:"freeCashFlowPerShare"`
	DividendPerShare     float64 `json:"dividendPerShare"`
	BookValuePerShare    float64 `json:"bookValuePerShare"`
	RevenueGrowthRate    float64 `json:"revenueGrowthRate"`
	EarningsGrowthRate   float64 `json:"earningsGrowthRate"`
	GrossMargin          float64 `json:"grossMargin"`
	OperatingMargin      float64 `json:"operatingMargin"`
	NetMargin            float64 `json:"netMargin"`
	ROE                  float64 `json:"roe"`
	ROIC                 float64 `json:"roic"`
	ROA                  float64 `json:"returnOnAssetCurrent"`
	DebtToEquityRatio    float64 `json:"debtToEquityRatio"`
	DebtToEBITDA         float64 `json:"debtToEbitda"`
	FinancialLeverage    float64 `json:"financialLeverage"`
	QuickRatio           float64 `json:"quickRatio"`
	CurrentRatio         float64 `json:"currentRatio"`
	AssetTurnover        float64 `json:"assetTurnover"`
	InventoryTurnover    float64 `json:"inventoryTurnover"`
	ReceivableTurnover   float64 `json:"receivableTurnover"`
	PayoutRatio          float64 `json:"payoutRatio"`
	PriceToSalesRatio    float64 `json:"priceToSalesRatio"`
	PriceToEarningsRatio float64 `json:"priceToEarningsRatio"`
	PriceToCashFlowRatio float64 `json:"priceToCashFlowRatio"`
	PriceToBookRatio     float64 `json:"priceToBookRatio"`
	EVToEBITDA           float64 `json:"evEbitda"`
}

// EarningsAPIResponse represents the actual API response from Finance/Events/Earnings
type EarningsAPIResponse struct {
	History struct {
		Quarterly map[string]EarningsData `json:"quarterly"`
		Annual    map[string]EarningsData `json:"annual"`
	} `json:"History"`
	InstrumentID string `json:"InstrumentId"`
	Symbol       string `json:"Symbol"`
}

// EarningsData represents a single earnings report from the API
type EarningsData struct {
	EpsActual           float64 `json:"EpsActual"`
	EpsSurprise         float64 `json:"EpsSurprise"`
	EpsSurprisePercent  float64 `json:"EpsSurprisePercent"`
	EpsForecast         float64 `json:"EpsForecast"`
	RevenueActual       float64 `json:"RevenueActual"`
	RevenueSurprise     float64 `json:"RevenueSurprise"`
	RevenueForecast     float64 `json:"RevenueForecast"`
	EarningReleaseDate  string  `json:"EarningReleaseDate"`
	CiqFiscalPeriodType string  `json:"CiqFiscalPeriodType"` // e.g., "Q42025", "Q12026"
	CalendarPeriodType  string  `json:"CalendarPeriodType"`
}

// EarningsEvent represents a normalized earnings event for storage
type EarningsEvent struct {
	ID              string  `json:"id"`
	EventDate       string  `json:"eventDate"`
	FiscalYear      int     `json:"fiscalYear"`
	FiscalQuarter   int     `json:"fiscalQuarter"`
	EPSEstimate     float64 `json:"epsEstimate"`
	EPSActual       float64 `json:"epsActual"`
	EPSSurprise     float64 `json:"epsSurprise"`
	EPSSurprisePct  float64 `json:"epsSurprisePercent"`
	RevenueEstimate float64 `json:"revenueEstimate"`
	RevenueActual   float64 `json:"revenueActual"`
	RevenueSurprise float64 `json:"revenueSurprise"`
}

// SentimentResponse from Finance/SentimentBrowser
type SentimentResponse []SentimentData

// SentimentData represents market sentiment for a stock
type SentimentData struct {
	DisplayName         string               `json:"displayName"`
	Market              string               `json:"market"`
	InstrumentID        string               `json:"instrumentId"`
	Symbol              string               `json:"symbol"`
	SentimentStatistics []SentimentStatistic `json:"sentimentStatistics"`
}

// SentimentStatistic represents sentiment data for a time period
type SentimentStatistic struct {
	StartTime      int64   `json:"startTime"`
	EndTime        int64   `json:"endTime"`
	TimeRangeName  string  `json:"timeRangeName"`
	TimeRangeEnum  string  `json:"timeRangeEnum"`
	Bullish        int     `json:"bullish"`
	Bearish        int     `json:"bearish"`
	Neutral        int     `json:"neutral"`
	BullishPercent float64 `json:"bullishPercent"`
	BearishPercent float64 `json:"bearishPercent"`
	NeutralPercent float64 `json:"neutralPercent"`
	Scenario       string  `json:"scenairo"` // Note: API has typo "scenairo"
}

// InsightsResponse from api.msn.com insights
type InsightsResponse []InsightData

// InsightData represents AI-generated stock insights
type InsightData struct {
	ID          string   `json:"id"`
	Summary     string   `json:"summary"`
	Highlights  []string `json:"highlights"`
	Risks       []string `json:"risks"`
	LastUpdated string   `json:"lastUpdated"`
}

// NewsFeedResponse from MSN/Feed/me
type NewsFeedResponse struct {
	Value    []NewsItem `json:"value"`
	SubCards []NewsItem `json:"subCards"`
}

// NewsItem represents a news article
type NewsItem struct {
	ID          string        `json:"id"`
	Type        string        `json:"type"`
	Title       string        `json:"title"`
	URL         string        `json:"url"`
	Description string        `json:"abstract"`
	Provider    *NewsProvider `json:"provider"`
	PublishTime string        `json:"publishedDateTime"`
	Images      []NewsImage   `json:"images"`
	ReadTimeMin int           `json:"readTimeMin"`
}

// NewsProvider represents a news provider
type NewsProvider struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// NewsImage represents a news article image
type NewsImage struct {
	URL    string `json:"url"`
	Width  int    `json:"width"`
	Height int    `json:"height"`
}

// Holder represents an institutional holder
type Holder struct {
	Name         string  `json:"investorName"`
	Type         string  `json:"investorType"`
	SharesHeld   int64   `json:"sharesHeld"`
	SharesChange int64   `json:"sharesChange"`
	SharesPct    float64 `json:"sharesPercent"`
	Value        float64 `json:"value"`
	ReportDate   string  `json:"reportDate"`
}

// OwnershipResponse from Bing API
type OwnershipResponse struct {
	Records            []Holder `json:"records"`
	SecurityOwnerships []Holder `json:"securityOwnerships"`
	Total              int      `json:"total"`
}

// OwnershipData aggregates all ownership information
type OwnershipData struct {
	TopHolders    []Holder `json:"top_holders"`
	TopBuyers     []Holder `json:"top_buyers"`
	TopSellers    []Holder `json:"top_sellers"`
	NewHolders    []Holder `json:"new_holders"`
	ExitedHolders []Holder `json:"exited_holders"`
}

// StockData is the complete stock information output
type StockData struct {
	ID       string `json:"id"`
	Ticker   string `json:"ticker"`
	Name     string `json:"name"`
	Exchange string `json:"exchange"`
	Sector   string `json:"sector"`
	Industry string `json:"industry"`

	// Real-time data
	Quote *QuoteData `json:"quote,omitempty"`

	// Historical Charts
	Charts map[string][]ChartPoint `json:"charts,omitempty"`

	// Fundamentals
	Financials *FinancialData `json:"financials,omitempty"`
	KeyRatios  *KeyRatios     `json:"key_ratios,omitempty"`

	// Company Info
	Company *EquityData `json:"company,omitempty"`

	// Events
	Earnings []EarningsEvent `json:"earnings,omitempty"`

	// Analysis
	Sentiment *SentimentData `json:"sentiment,omitempty"`
	Insights  *InsightData   `json:"insights,omitempty"`

	// Ownership (Bing API)
	Ownership *OwnershipData `json:"ownership,omitempty"`

	// News
	News []NewsItem `json:"news,omitempty"`

	// Metadata
	FetchedAt   string            `json:"fetched_at"`
	FetchStatus map[string]string `json:"fetch_status"`
}

// FinancialData aggregates all financial statements
type FinancialData struct {
	Statements []FinancialStatement `json:"statements,omitempty"`
}

// ScreenerOutput is the JSON output for screener command
type ScreenerOutput struct {
	Filter      string          `json:"filter"`
	Region      string          `json:"region"`
	GeneratedAt string          `json:"generated_at"`
	Total       int             `json:"total"`
	Stocks      []ScreenerStock `json:"stocks"`
}

// FetchOutput is the JSON output for fetch command
type FetchOutput struct {
	GeneratedAt string      `json:"generated_at"`
	Total       int         `json:"total"`
	Stocks      []StockData `json:"stocks"`
}
