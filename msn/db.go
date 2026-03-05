package msn

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

// StockDB manages SQLite database operations
type StockDB struct {
	db *sql.DB
}

// NewStockDB creates a new database connection and initializes schema
func NewStockDB(dbPath string) (*StockDB, error) {
	db, err := sql.Open("sqlite3", dbPath+"?_journal_mode=WAL&_synchronous=NORMAL")
	if err != nil {
		return nil, fmt.Errorf("failed to open database: %w", err)
	}

	sdb := &StockDB{db: db}
	if err := sdb.initSchema(); err != nil {
		db.Close()
		return nil, fmt.Errorf("failed to initialize schema: %w", err)
	}

	return sdb, nil
}

// Close closes the database connection
func (s *StockDB) Close() error {
	return s.db.Close()
}

// initSchema creates all database tables
func (s *StockDB) initSchema() error {
	schema := `
	-- Core stock info (updated each run)
	CREATE TABLE IF NOT EXISTS stocks (
		id TEXT PRIMARY KEY,
		ticker TEXT NOT NULL,
		name TEXT,
		display_name TEXT,
		sector TEXT,
		industry TEXT,
		exchange_id TEXT,
		exchange_code TEXT,
		exchange_name TEXT,
		country TEXT,
		currency TEXT,
		market TEXT,
		website TEXT,
		employees INTEGER,
		description TEXT,
		address TEXT,
		city TEXT,
		phone TEXT,
		last_updated DATETIME,
		last_seen DATE,
		created_at DATETIME DEFAULT CURRENT_TIMESTAMP
	);
	CREATE INDEX IF NOT EXISTS idx_stocks_ticker ON stocks(ticker);
	CREATE INDEX IF NOT EXISTS idx_stocks_sector ON stocks(sector);
	CREATE INDEX IF NOT EXISTS idx_stocks_last_seen ON stocks(last_seen);

	-- Price snapshots (historical - one row per stock per day)
	CREATE TABLE IF NOT EXISTS price_history (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		scrape_date DATE NOT NULL,
		price REAL,
		price_change REAL,
		price_change_pct REAL,
		price_open REAL,
		price_high REAL,
		price_low REAL,
		price_close REAL,
		price_prev_close REAL,
		price_52w_high REAL,
		price_52w_low REAL,
		volume REAL,
		avg_volume REAL,
		market_cap REAL,
		market_cap_currency TEXT,
		price_change_1w REAL,
		price_change_1m REAL,
		price_change_3m REAL,
		price_change_6m REAL,
		price_change_ytd REAL,
		price_change_1y REAL,
		return_1w REAL,
		return_1m REAL,
		return_3m REAL,
		return_6m REAL,
		return_ytd REAL,
		return_1y REAL,
		time_last_traded TEXT,
		UNIQUE(stock_id, scrape_date),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_price_history_stock ON price_history(stock_id);
	CREATE INDEX IF NOT EXISTS idx_price_history_date ON price_history(scrape_date);

	-- Financial ratios (historical - per year per stock)
	CREATE TABLE IF NOT EXISTS ratios_history (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		scrape_date DATE NOT NULL,
		year TEXT,
		fiscal_period TEXT,
		pe_ratio REAL,
		pb_ratio REAL,
		ps_ratio REAL,
		pcf_ratio REAL,
		ev_ebitda REAL,
		dividend_yield REAL,
		payout_ratio REAL,
		roe REAL,
		roa REAL,
		roic REAL,
		gross_margin REAL,
		operating_margin REAL,
		net_margin REAL,
		debt_to_equity REAL,
		debt_to_ebitda REAL,
		financial_leverage REAL,
		current_ratio REAL,
		quick_ratio REAL,
		asset_turnover REAL,
		inventory_turnover REAL,
		receivable_turnover REAL,
		revenue_growth REAL,
		earnings_growth REAL,
		eps REAL,
		bvps REAL,
		revenue_per_share REAL,
		fcf_per_share REAL,
		dividend_per_share REAL,
		UNIQUE(stock_id, scrape_date, year),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_ratios_history_stock ON ratios_history(stock_id);

	-- Balance sheets
	CREATE TABLE IF NOT EXISTS balance_sheets (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		report_date DATE,
		end_date DATE,
		source TEXT,
		source_date DATE,
		current_assets_json TEXT,
		long_term_assets_json TEXT,
		current_liabilities_json TEXT,
		equity_json TEXT,
		currency TEXT,
		scrape_date DATE NOT NULL,
		UNIQUE(stock_id, end_date),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);

	-- Cash flows
	CREATE TABLE IF NOT EXISTS cash_flows (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		report_date DATE,
		end_date DATE,
		source TEXT,
		operating_json TEXT,
		investing_json TEXT,
		financing_json TEXT,
		currency TEXT,
		scrape_date DATE NOT NULL,
		UNIQUE(stock_id, end_date),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);

	-- Income statements
	CREATE TABLE IF NOT EXISTS income_statements (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		report_date DATE,
		end_date DATE,
		source TEXT,
		revenue_json TEXT,
		expenses_json TEXT,
		currency TEXT,
		scrape_date DATE NOT NULL,
		UNIQUE(stock_id, end_date),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);

	-- Earnings events
	CREATE TABLE IF NOT EXISTS earnings (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		event_date DATE,
		fiscal_year INTEGER,
		fiscal_quarter INTEGER,
		eps_estimate REAL,
		eps_actual REAL,
		eps_surprise REAL,
		eps_surprise_pct REAL,
		revenue_estimate REAL,
		revenue_actual REAL,
		revenue_surprise REAL,
		scrape_date DATE NOT NULL,
		UNIQUE(stock_id, event_date, fiscal_year, fiscal_quarter),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_earnings_stock ON earnings(stock_id);

	-- Chart OHLCV data
	CREATE TABLE IF NOT EXISTS charts (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		timeframe TEXT NOT NULL,
		timestamp DATETIME NOT NULL,
		open REAL,
		high REAL,
		low REAL,
		close REAL,
		volume INTEGER,
		scrape_date DATE NOT NULL,
		UNIQUE(stock_id, timeframe, timestamp),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_charts_stock_tf ON charts(stock_id, timeframe);

	-- News articles
	CREATE TABLE IF NOT EXISTS news (
		id TEXT PRIMARY KEY,
		stock_id TEXT NOT NULL,
		title TEXT,
		url TEXT,
		abstract TEXT,
		source_id TEXT,
		source_name TEXT,
		published_at DATETIME,
		read_time_min INTEGER,
		image_url TEXT,
		image_width INTEGER,
		image_height INTEGER,
		news_type TEXT,
		category TEXT,
		sentiment TEXT,
		sentiment_score REAL,
		is_critical INTEGER DEFAULT 0,
		first_seen DATE NOT NULL,
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_news_stock ON news(stock_id);
	CREATE INDEX IF NOT EXISTS idx_news_published ON news(published_at);
	CREATE INDEX IF NOT EXISTS idx_news_category ON news(category);
	CREATE INDEX IF NOT EXISTS idx_news_critical ON news(is_critical);

	-- Sentiment data (historical)
	CREATE TABLE IF NOT EXISTS sentiment_history (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		scrape_date DATE NOT NULL,
		time_range TEXT,
		time_range_enum TEXT,
		start_time INTEGER,
		end_time INTEGER,
		bullish INTEGER,
		bearish INTEGER,
		neutral INTEGER,
		bullish_pct REAL,
		bearish_pct REAL,
		neutral_pct REAL,
		scenario TEXT,
		UNIQUE(stock_id, scrape_date, time_range_enum),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_sentiment_stock ON sentiment_history(stock_id);

	-- AI Insights
	CREATE TABLE IF NOT EXISTS insights (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		scrape_date DATE NOT NULL,
		summary TEXT,
		highlights_json TEXT,
		risks_json TEXT,
		last_updated TEXT,
		UNIQUE(stock_id, scrape_date),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);

	-- Ownership data
	CREATE TABLE IF NOT EXISTS ownership (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		scrape_date DATE NOT NULL,
		holder_type TEXT,
		investor_name TEXT,
		investor_type TEXT,
		shares_held INTEGER,
		shares_change INTEGER,
		shares_pct REAL,
		value REAL,
		report_date DATE,
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_ownership_stock ON ownership(stock_id);

	-- Company officers
	CREATE TABLE IF NOT EXISTS officers (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		stock_id TEXT NOT NULL,
		name TEXT,
		title TEXT,
		age INTEGER,
		year_born INTEGER,
		total_pay INTEGER,
		as_of_date DATE,
		scrape_date DATE NOT NULL,
		UNIQUE(stock_id, name, title),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);

	-- Scrape runs tracking
	CREATE TABLE IF NOT EXISTS scrape_runs (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		started_at DATETIME NOT NULL,
		completed_at DATETIME,
		status TEXT NOT NULL DEFAULT 'running',
		total_stocks INTEGER,
		successful INTEGER DEFAULT 0,
		failed INTEGER DEFAULT 0,
		config_json TEXT
	);

	-- Progress tracking (for resume)
	CREATE TABLE IF NOT EXISTS scrape_progress (
		run_id INTEGER NOT NULL,
		stock_id TEXT NOT NULL,
		ticker TEXT,
		status TEXT NOT NULL DEFAULT 'pending',
		error_message TEXT,
		apis_success INTEGER DEFAULT 0,
		apis_failed INTEGER DEFAULT 0,
		started_at DATETIME,
		completed_at DATETIME,
		PRIMARY KEY(run_id, stock_id),
		FOREIGN KEY(run_id) REFERENCES scrape_runs(id)
	);
	CREATE INDEX IF NOT EXISTS idx_progress_status ON scrape_progress(run_id, status);

	-- Stock indices (LQ45, IDX80, etc.)
	CREATE TABLE IF NOT EXISTS stock_indices (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		index_name TEXT NOT NULL,
		stock_id TEXT NOT NULL,
		ticker TEXT NOT NULL,
		added_date DATE,
		UNIQUE(index_name, stock_id),
		FOREIGN KEY(stock_id) REFERENCES stocks(id)
	);
	CREATE INDEX IF NOT EXISTS idx_stock_indices_name ON stock_indices(index_name);
	`

	_, err := s.db.Exec(schema)
	return err
}

// StartScrapeRun creates a new scrape run and returns its ID
func (s *StockDB) StartScrapeRun(totalStocks int, config map[string]interface{}) (int64, error) {
	configJSON, _ := json.Marshal(config)

	result, err := s.db.Exec(`
		INSERT INTO scrape_runs (started_at, status, total_stocks, config_json)
		VALUES (?, 'running', ?, ?)
	`, time.Now().UTC(), totalStocks, string(configJSON))

	if err != nil {
		return 0, err
	}

	return result.LastInsertId()
}

// InitProgress initializes progress for all stocks in a run
func (s *StockDB) InitProgress(runID int64, stocks map[string]IDXStock) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()

	stmt, err := tx.Prepare(`
		INSERT OR IGNORE INTO scrape_progress (run_id, stock_id, ticker, status)
		VALUES (?, ?, ?, 'pending')
	`)
	if err != nil {
		return err
	}
	defer stmt.Close()

	for ticker, stock := range stocks {
		_, err := stmt.Exec(runID, stock.ID, ticker)
		if err != nil {
			return err
		}
	}

	return tx.Commit()
}

// GetPendingStocks returns stocks that haven't been processed in this run
func (s *StockDB) GetPendingStocks(runID int64) ([]struct {
	ID     string
	Ticker string
}, error) {
	rows, err := s.db.Query(`
		SELECT stock_id, ticker FROM scrape_progress
		WHERE run_id = ? AND status = 'pending'
		ORDER BY ticker
	`, runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var stocks []struct {
		ID     string
		Ticker string
	}

	for rows.Next() {
		var stock struct {
			ID     string
			Ticker string
		}
		if err := rows.Scan(&stock.ID, &stock.Ticker); err != nil {
			return nil, err
		}
		stocks = append(stocks, stock)
	}

	return stocks, nil
}

// UpdateProgress updates the progress of a stock
func (s *StockDB) UpdateProgress(runID int64, stockID string, status string, apisSuccess, apisFailed int, errMsg string) error {
	_, err := s.db.Exec(`
		UPDATE scrape_progress
		SET status = ?, apis_success = ?, apis_failed = ?, error_message = ?, completed_at = ?
		WHERE run_id = ? AND stock_id = ?
	`, status, apisSuccess, apisFailed, errMsg, time.Now().UTC(), runID, stockID)
	return err
}

// MarkProgressStarted marks a stock as started
func (s *StockDB) MarkProgressStarted(runID int64, stockID string) error {
	_, err := s.db.Exec(`
		UPDATE scrape_progress SET status = 'processing', started_at = ?
		WHERE run_id = ? AND stock_id = ?
	`, time.Now().UTC(), runID, stockID)
	return err
}

// CompleteScrapeRun marks a scrape run as completed
func (s *StockDB) CompleteScrapeRun(runID int64, status string) error {
	// Count successful and failed
	var successful, failed int
	s.db.QueryRow(`SELECT COUNT(*) FROM scrape_progress WHERE run_id = ? AND status = 'success'`, runID).Scan(&successful)
	s.db.QueryRow(`SELECT COUNT(*) FROM scrape_progress WHERE run_id = ? AND status = 'failed'`, runID).Scan(&failed)

	_, err := s.db.Exec(`
		UPDATE scrape_runs SET completed_at = ?, status = ?, successful = ?, failed = ?
		WHERE id = ?
	`, time.Now().UTC(), status, successful, failed, runID)
	return err
}

// GetLastRunID returns the most recent incomplete run ID (for resume)
func (s *StockDB) GetLastRunID() (int64, error) {
	var runID int64
	err := s.db.QueryRow(`
		SELECT id FROM scrape_runs
		WHERE status IN ('running', 'interrupted')
		ORDER BY started_at DESC LIMIT 1
	`).Scan(&runID)

	if err == sql.ErrNoRows {
		return 0, nil
	}
	return runID, err
}

// SaveStockData saves all stock data to the database
func (s *StockDB) SaveStockData(stock *StockData) error {
	today := time.Now().UTC().Format("2006-01-02")

	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()

	// Save core stock info
	if err := s.saveStock(tx, stock, today); err != nil {
		return fmt.Errorf("save stock: %w", err)
	}

	// Save price history
	if stock.Quote != nil {
		if err := s.savePriceHistory(tx, stock, today); err != nil {
			return fmt.Errorf("save price: %w", err)
		}
	}

	// Save ratios
	if stock.KeyRatios != nil {
		if err := s.saveRatios(tx, stock, today); err != nil {
			return fmt.Errorf("save ratios: %w", err)
		}
	}

	// Save financials
	if stock.Financials != nil {
		if err := s.saveFinancials(tx, stock, today); err != nil {
			return fmt.Errorf("save financials: %w", err)
		}
	}

	// Save earnings
	if len(stock.Earnings) > 0 {
		if err := s.saveEarnings(tx, stock, today); err != nil {
			return fmt.Errorf("save earnings: %w", err)
		}
	}

	// Save charts
	if len(stock.Charts) > 0 {
		if err := s.saveCharts(tx, stock, today); err != nil {
			return fmt.Errorf("save charts: %w", err)
		}
	}

	// Save news
	if len(stock.News) > 0 {
		if err := s.saveNews(tx, stock, today); err != nil {
			return fmt.Errorf("save news: %w", err)
		}
	}

	// Save sentiment
	if stock.Sentiment != nil {
		if err := s.saveSentiment(tx, stock, today); err != nil {
			return fmt.Errorf("save sentiment: %w", err)
		}
	}

	// Save insights
	if stock.Insights != nil {
		if err := s.saveInsights(tx, stock, today); err != nil {
			return fmt.Errorf("save insights: %w", err)
		}
	}

	// Save ownership
	if stock.Ownership != nil {
		if err := s.saveOwnership(tx, stock, today); err != nil {
			return fmt.Errorf("save ownership: %w", err)
		}
	}

	return tx.Commit()
}

func (s *StockDB) saveStock(tx *sql.Tx, stock *StockData, today string) error {
	var website, description, address, city, phone string
	var employees int

	if stock.Company != nil {
		website = stock.Company.Website
		description = stock.Company.Description
		address = stock.Company.Address
		city = stock.Company.City
		phone = stock.Company.Phone
		employees = stock.Company.Employees
	}

	var exchangeCode, exchangeName, country, currency, market, displayName string
	if stock.Quote != nil {
		exchangeCode = stock.Quote.ExchangeCode
		exchangeName = stock.Quote.ExchangeName
		country = stock.Quote.Country
		currency = stock.Quote.Currency
		market = stock.Quote.Market
		displayName = stock.Quote.DisplayName
	}

	_, err := tx.Exec(`
		INSERT INTO stocks (id, ticker, name, display_name, sector, industry,
			exchange_id, exchange_code, exchange_name, country, currency, market,
			website, employees, description, address, city, phone, last_updated, last_seen)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			ticker = excluded.ticker,
			name = excluded.name,
			display_name = excluded.display_name,
			sector = COALESCE(excluded.sector, sector),
			industry = COALESCE(excluded.industry, industry),
			exchange_id = COALESCE(excluded.exchange_id, exchange_id),
			exchange_code = COALESCE(excluded.exchange_code, exchange_code),
			exchange_name = COALESCE(excluded.exchange_name, exchange_name),
			country = COALESCE(excluded.country, country),
			currency = COALESCE(excluded.currency, currency),
			market = COALESCE(excluded.market, market),
			website = COALESCE(excluded.website, website),
			employees = COALESCE(excluded.employees, employees),
			description = COALESCE(excluded.description, description),
			address = COALESCE(excluded.address, address),
			city = COALESCE(excluded.city, city),
			phone = COALESCE(excluded.phone, phone),
			last_updated = excluded.last_updated,
			last_seen = excluded.last_seen
	`, stock.ID, stock.Ticker, stock.Name, displayName, stock.Sector, stock.Industry,
		stock.Exchange, exchangeCode, exchangeName, country, currency, market,
		website, employees, description, address, city, phone, stock.FetchedAt, today)

	return err
}

func (s *StockDB) savePriceHistory(tx *sql.Tx, stock *StockData, today string) error {
	q := stock.Quote
	_, err := tx.Exec(`
		INSERT INTO price_history (stock_id, scrape_date, price, price_change, price_change_pct,
			price_open, price_high, price_low, price_close, price_prev_close,
			price_52w_high, price_52w_low, volume, avg_volume, market_cap, market_cap_currency,
			price_change_1w, price_change_1m, price_change_3m, price_change_6m, price_change_ytd, price_change_1y,
			return_1w, return_1m, return_3m, return_6m, return_ytd, return_1y, time_last_traded)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(stock_id, scrape_date) DO UPDATE SET
			price = excluded.price,
			price_change = excluded.price_change,
			price_change_pct = excluded.price_change_pct,
			price_open = excluded.price_open,
			price_high = excluded.price_high,
			price_low = excluded.price_low,
			price_close = excluded.price_close,
			price_prev_close = excluded.price_prev_close,
			price_52w_high = excluded.price_52w_high,
			price_52w_low = excluded.price_52w_low,
			volume = excluded.volume,
			avg_volume = excluded.avg_volume,
			market_cap = excluded.market_cap,
			market_cap_currency = excluded.market_cap_currency,
			price_change_1w = excluded.price_change_1w,
			price_change_1m = excluded.price_change_1m,
			price_change_3m = excluded.price_change_3m,
			price_change_6m = excluded.price_change_6m,
			price_change_ytd = excluded.price_change_ytd,
			price_change_1y = excluded.price_change_1y,
			return_1w = excluded.return_1w,
			return_1m = excluded.return_1m,
			return_3m = excluded.return_3m,
			return_6m = excluded.return_6m,
			return_ytd = excluded.return_ytd,
			return_1y = excluded.return_1y,
			time_last_traded = excluded.time_last_traded
	`, stock.ID, today, q.Price, q.PriceChange, q.PriceChangePct,
		q.PriceDayOpen, q.PriceDayHigh, q.PriceDayLow, q.PriceClose, q.PricePreviousClose,
		q.Price52wHigh, q.Price52wLow, q.AccumulatedVolume, q.AverageVolume, q.MarketCap, q.MarketCapCurrency,
		q.PriceChange1Week, q.PriceChange1Month, q.PriceChange3Month, q.PriceChange6Month, q.PriceChangeYTD, q.PriceChange1Year,
		q.Return1Week, q.Return1Month, q.Return3Month, q.Return6Month, q.ReturnYTD, q.Return1Year, q.TimeLastTraded)

	return err
}

func (s *StockDB) saveRatios(tx *sql.Tx, stock *StockData, today string) error {
	// Get current price for dividend yield calculation
	var currentPrice float64
	if stock.Quote != nil && stock.Quote.Price > 0 {
		currentPrice = stock.Quote.Price
	}

	for _, metric := range stock.KeyRatios.IndustryMetrics {
		// Calculate dividend yield: (DividendPerShare / Price) * 100
		var dividendYield float64
		if currentPrice > 0 && metric.DividendPerShare > 0 {
			dividendYield = (metric.DividendPerShare / currentPrice) * 100
		}

		_, err := tx.Exec(`
			INSERT INTO ratios_history (stock_id, scrape_date, year, fiscal_period,
				pe_ratio, pb_ratio, ps_ratio, pcf_ratio, ev_ebitda,
				dividend_yield, payout_ratio, roe, roa, roic,
				gross_margin, operating_margin, net_margin,
				debt_to_equity, debt_to_ebitda, financial_leverage,
				current_ratio, quick_ratio, asset_turnover, inventory_turnover, receivable_turnover,
				revenue_growth, earnings_growth, eps, bvps, revenue_per_share, fcf_per_share, dividend_per_share)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
			ON CONFLICT(stock_id, scrape_date, year) DO UPDATE SET
				fiscal_period = excluded.fiscal_period,
				pe_ratio = excluded.pe_ratio,
				pb_ratio = excluded.pb_ratio,
				ps_ratio = excluded.ps_ratio,
				pcf_ratio = excluded.pcf_ratio,
				ev_ebitda = excluded.ev_ebitda,
				dividend_yield = excluded.dividend_yield,
				payout_ratio = excluded.payout_ratio,
				roe = excluded.roe,
				roa = excluded.roa,
				roic = excluded.roic,
				gross_margin = excluded.gross_margin,
				operating_margin = excluded.operating_margin,
				net_margin = excluded.net_margin,
				debt_to_equity = excluded.debt_to_equity,
				debt_to_ebitda = excluded.debt_to_ebitda,
				financial_leverage = excluded.financial_leverage,
				current_ratio = excluded.current_ratio,
				quick_ratio = excluded.quick_ratio,
				asset_turnover = excluded.asset_turnover,
				inventory_turnover = excluded.inventory_turnover,
				receivable_turnover = excluded.receivable_turnover,
				revenue_growth = excluded.revenue_growth,
				earnings_growth = excluded.earnings_growth,
				eps = excluded.eps,
				bvps = excluded.bvps,
				revenue_per_share = excluded.revenue_per_share,
				fcf_per_share = excluded.fcf_per_share,
				dividend_per_share = excluded.dividend_per_share
		`, stock.ID, today, metric.Year, metric.FiscalPeriodType,
			metric.PriceToEarningsRatio, metric.PriceToBookRatio, metric.PriceToSalesRatio, metric.PriceToCashFlowRatio, metric.EVToEBITDA,
			dividendYield, metric.PayoutRatio, metric.ROE, metric.ROA, metric.ROIC,
			metric.GrossMargin, metric.OperatingMargin, metric.NetMargin,
			metric.DebtToEquityRatio, metric.DebtToEBITDA, metric.FinancialLeverage,
			metric.CurrentRatio, metric.QuickRatio, metric.AssetTurnover, metric.InventoryTurnover, metric.ReceivableTurnover,
			metric.RevenueGrowthRate, metric.EarningsGrowthRate, metric.EarningsPerShare, metric.BookValuePerShare,
			metric.RevenuePerShare, metric.FreeCashFlowPerShare, metric.DividendPerShare)

		if err != nil {
			return err
		}
	}
	return nil
}

func (s *StockDB) saveFinancials(tx *sql.Tx, stock *StockData, today string) error {
	for _, stmt := range stock.Financials.Statements {
		// Save balance sheet
		if stmt.BalanceSheets != nil {
			currentAssetsJSON, _ := json.Marshal(stmt.BalanceSheets.CurrentAssets)
			longTermAssetsJSON, _ := json.Marshal(stmt.BalanceSheets.LongTermAssets)
			currentLiabilitiesJSON, _ := json.Marshal(stmt.BalanceSheets.CurrentLiabilities)
			equityJSON, _ := json.Marshal(stmt.BalanceSheets.Equity)

			_, err := tx.Exec(`
				INSERT INTO balance_sheets (stock_id, report_date, end_date, source, source_date,
					current_assets_json, long_term_assets_json, current_liabilities_json, equity_json, currency, scrape_date)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
				ON CONFLICT(stock_id, end_date) DO UPDATE SET
					report_date = excluded.report_date,
					source = excluded.source,
					source_date = excluded.source_date,
					current_assets_json = excluded.current_assets_json,
					long_term_assets_json = excluded.long_term_assets_json,
					current_liabilities_json = excluded.current_liabilities_json,
					equity_json = excluded.equity_json,
					currency = excluded.currency,
					scrape_date = excluded.scrape_date
			`, stock.ID, stmt.BalanceSheets.ReportDate, stmt.BalanceSheets.EndDate, stmt.BalanceSheets.Source, stmt.BalanceSheets.SourceDate,
				string(currentAssetsJSON), string(longTermAssetsJSON), string(currentLiabilitiesJSON), string(equityJSON),
				stmt.BalanceSheets.Currency, today)

			if err != nil {
				return err
			}
		}

		// Save cash flow
		if stmt.CashFlow != nil {
			operatingJSON, _ := json.Marshal(stmt.CashFlow.Operating)
			investingJSON, _ := json.Marshal(stmt.CashFlow.Investing)
			financingJSON, _ := json.Marshal(stmt.CashFlow.Financing)

			_, err := tx.Exec(`
				INSERT INTO cash_flows (stock_id, end_date, source, operating_json, investing_json, financing_json, currency, scrape_date)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?)
				ON CONFLICT(stock_id, end_date) DO UPDATE SET
					source = excluded.source,
					operating_json = excluded.operating_json,
					investing_json = excluded.investing_json,
					financing_json = excluded.financing_json,
					currency = excluded.currency,
					scrape_date = excluded.scrape_date
			`, stock.ID, stmt.CashFlow.EndDate, stmt.CashFlow.Source,
				string(operatingJSON), string(investingJSON), string(financingJSON),
				stmt.CashFlow.Currency, today)

			if err != nil {
				return err
			}
		}

		// Save income statement
		if stmt.IncomeStatements != nil {
			revenueJSON, _ := json.Marshal(stmt.IncomeStatements.Revenue)
			expensesJSON, _ := json.Marshal(stmt.IncomeStatements.Expenses)

			_, err := tx.Exec(`
				INSERT INTO income_statements (stock_id, end_date, source, revenue_json, expenses_json, currency, scrape_date)
				VALUES (?, ?, ?, ?, ?, ?, ?)
				ON CONFLICT(stock_id, end_date) DO UPDATE SET
					source = excluded.source,
					revenue_json = excluded.revenue_json,
					expenses_json = excluded.expenses_json,
					currency = excluded.currency,
					scrape_date = excluded.scrape_date
			`, stock.ID, stmt.IncomeStatements.EndDate, stmt.IncomeStatements.Source,
				string(revenueJSON), string(expensesJSON), stmt.IncomeStatements.Currency, today)

			if err != nil {
				return err
			}
		}
	}
	return nil
}

func (s *StockDB) saveEarnings(tx *sql.Tx, stock *StockData, today string) error {
	for _, e := range stock.Earnings {
		_, err := tx.Exec(`
			INSERT INTO earnings (stock_id, event_date, fiscal_year, fiscal_quarter,
				eps_estimate, eps_actual, eps_surprise, eps_surprise_pct,
				revenue_estimate, revenue_actual, revenue_surprise, scrape_date)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
			ON CONFLICT(stock_id, event_date, fiscal_year, fiscal_quarter) DO UPDATE SET
				eps_estimate = excluded.eps_estimate,
				eps_actual = excluded.eps_actual,
				eps_surprise = excluded.eps_surprise,
				eps_surprise_pct = excluded.eps_surprise_pct,
				revenue_estimate = excluded.revenue_estimate,
				revenue_actual = excluded.revenue_actual,
				revenue_surprise = excluded.revenue_surprise,
				scrape_date = excluded.scrape_date
		`, stock.ID, e.EventDate, e.FiscalYear, e.FiscalQuarter,
			e.EPSEstimate, e.EPSActual, e.EPSSurprise, e.EPSSurprisePct,
			e.RevenueEstimate, e.RevenueActual, e.RevenueSurprise, today)

		if err != nil {
			return err
		}
	}
	return nil
}

func (s *StockDB) saveCharts(tx *sql.Tx, stock *StockData, today string) error {
	for timeframe, points := range stock.Charts {
		for _, p := range points {
			_, err := tx.Exec(`
				INSERT INTO charts (stock_id, timeframe, timestamp, open, high, low, close, volume, scrape_date)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
				ON CONFLICT(stock_id, timeframe, timestamp) DO UPDATE SET
					open = excluded.open,
					high = excluded.high,
					low = excluded.low,
					close = excluded.close,
					volume = excluded.volume,
					scrape_date = excluded.scrape_date
			`, stock.ID, timeframe, p.Time, p.Open, p.High, p.Low, p.Close, p.Volume, today)

			if err != nil {
				return err
			}
		}
	}
	return nil
}

func (s *StockDB) saveNews(tx *sql.Tx, stock *StockData, today string) error {
	for _, n := range stock.News {
		var sourceID, sourceName, imageURL string
		var imageWidth, imageHeight int

		if n.Provider != nil {
			sourceID = n.Provider.ID
			sourceName = n.Provider.Name
		}
		if len(n.Images) > 0 {
			imageURL = n.Images[0].URL
			imageWidth = n.Images[0].Width
			imageHeight = n.Images[0].Height
		}

		// Categorize and score sentiment
		category := categorizeNews(n.Title, n.Description)
		sentiment, sentimentScore := scoreNewsSentiment(n.Title, n.Description)
		isCritical := isNewsCritical(n.Title, n.Description)

		_, err := tx.Exec(`
			INSERT INTO news (id, stock_id, title, url, abstract, source_id, source_name,
				published_at, read_time_min, image_url, image_width, image_height, news_type,
				category, sentiment, sentiment_score, is_critical, first_seen)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				title = excluded.title,
				abstract = excluded.abstract,
				category = excluded.category,
				sentiment = excluded.sentiment,
				sentiment_score = excluded.sentiment_score,
				is_critical = excluded.is_critical
		`, n.ID, stock.ID, n.Title, n.URL, n.Description, sourceID, sourceName,
			n.PublishTime, n.ReadTimeMin, imageURL, imageWidth, imageHeight, n.Type,
			category, sentiment, sentimentScore, isCritical, today)

		if err != nil {
			return err
		}
	}
	return nil
}

func (s *StockDB) saveSentiment(tx *sql.Tx, stock *StockData, today string) error {
	for _, stat := range stock.Sentiment.SentimentStatistics {
		_, err := tx.Exec(`
			INSERT INTO sentiment_history (stock_id, scrape_date, time_range, time_range_enum,
				start_time, end_time, bullish, bearish, neutral, bullish_pct, bearish_pct, neutral_pct, scenario)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
			ON CONFLICT(stock_id, scrape_date, time_range_enum) DO UPDATE SET
				time_range = excluded.time_range,
				start_time = excluded.start_time,
				end_time = excluded.end_time,
				bullish = excluded.bullish,
				bearish = excluded.bearish,
				neutral = excluded.neutral,
				bullish_pct = excluded.bullish_pct,
				bearish_pct = excluded.bearish_pct,
				neutral_pct = excluded.neutral_pct,
				scenario = excluded.scenario
		`, stock.ID, today, stat.TimeRangeName, stat.TimeRangeEnum,
			stat.StartTime, stat.EndTime, stat.Bullish, stat.Bearish, stat.Neutral,
			stat.BullishPercent, stat.BearishPercent, stat.NeutralPercent, stat.Scenario)

		if err != nil {
			return err
		}
	}
	return nil
}

func (s *StockDB) saveInsights(tx *sql.Tx, stock *StockData, today string) error {
	highlightsJSON, _ := json.Marshal(stock.Insights.Highlights)
	risksJSON, _ := json.Marshal(stock.Insights.Risks)

	_, err := tx.Exec(`
		INSERT INTO insights (stock_id, scrape_date, summary, highlights_json, risks_json, last_updated)
		VALUES (?, ?, ?, ?, ?, ?)
		ON CONFLICT(stock_id, scrape_date) DO UPDATE SET
			summary = excluded.summary,
			highlights_json = excluded.highlights_json,
			risks_json = excluded.risks_json,
			last_updated = excluded.last_updated
	`, stock.ID, today, stock.Insights.Summary, string(highlightsJSON), string(risksJSON), stock.Insights.LastUpdated)

	return err
}

func (s *StockDB) saveOwnership(tx *sql.Tx, stock *StockData, today string) error {
	saveHolders := func(holders []Holder, holderType string) error {
		for _, h := range holders {
			_, err := tx.Exec(`
				INSERT INTO ownership (stock_id, scrape_date, holder_type, investor_name, investor_type,
					shares_held, shares_change, shares_pct, value, report_date)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
			`, stock.ID, today, holderType, h.Name, h.Type,
				h.SharesHeld, h.SharesChange, h.SharesPct, h.Value, h.ReportDate)

			if err != nil {
				return err
			}
		}
		return nil
	}

	if stock.Ownership.TopHolders != nil {
		if err := saveHolders(stock.Ownership.TopHolders, "top_holders"); err != nil {
			return err
		}
	}
	if stock.Ownership.TopBuyers != nil {
		if err := saveHolders(stock.Ownership.TopBuyers, "top_buyers"); err != nil {
			return err
		}
	}
	if stock.Ownership.TopSellers != nil {
		if err := saveHolders(stock.Ownership.TopSellers, "top_sellers"); err != nil {
			return err
		}
	}
	if stock.Ownership.NewHolders != nil {
		if err := saveHolders(stock.Ownership.NewHolders, "new_holders"); err != nil {
			return err
		}
	}
	if stock.Ownership.ExitedHolders != nil {
		if err := saveHolders(stock.Ownership.ExitedHolders, "exited_holders"); err != nil {
			return err
		}
	}

	return nil
}

// GetStaleStocks returns stocks not seen in the last N days
func (s *StockDB) GetStaleStocks(days int) ([]string, error) {
	cutoff := time.Now().AddDate(0, 0, -days).Format("2006-01-02")

	rows, err := s.db.Query(`
		SELECT ticker FROM stocks
		WHERE last_seen < ? OR last_seen IS NULL
		ORDER BY ticker
	`, cutoff)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var tickers []string
	for rows.Next() {
		var ticker string
		if err := rows.Scan(&ticker); err != nil {
			return nil, err
		}
		tickers = append(tickers, ticker)
	}
	return tickers, nil
}

// GetRunStats returns statistics for a run
func (s *StockDB) GetRunStats(runID int64) (pending, processing, success, failed int, err error) {
	err = s.db.QueryRow(`SELECT COUNT(*) FROM scrape_progress WHERE run_id = ? AND status = 'pending'`, runID).Scan(&pending)
	if err != nil {
		return
	}
	err = s.db.QueryRow(`SELECT COUNT(*) FROM scrape_progress WHERE run_id = ? AND status = 'processing'`, runID).Scan(&processing)
	if err != nil {
		return
	}
	err = s.db.QueryRow(`SELECT COUNT(*) FROM scrape_progress WHERE run_id = ? AND status = 'success'`, runID).Scan(&success)
	if err != nil {
		return
	}
	err = s.db.QueryRow(`SELECT COUNT(*) FROM scrape_progress WHERE run_id = ? AND status = 'failed'`, runID).Scan(&failed)
	return
}
