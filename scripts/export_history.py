#!/usr/bin/env python3
"""
History Excel Export Script - Professional Edition

Exports historical data (price history, ratio history, sentiment history) to Excel
with proper tables and conditional formatting. No charts - tables only.

Usage:
    uv run python scripts/export_history.py --db output/stocks.db --output output/history.xlsx
"""

import argparse
import sqlite3
from datetime import datetime
from pathlib import Path
from typing import Optional

import xlsxwriter
from xlsxwriter.utility import xl_range

# Color palette - matches export_dashboard.py
COLORS = {
    'primary_dark': '#1F4E79',
    'primary': '#2E75B6',
    'primary_light': '#5B9BD5',
    'success_dark': '#375623',
    'success': '#70AD47',
    'success_light': '#C6EFCE',
    'success_text': '#006100',
    'danger_dark': '#833C0C',
    'danger': '#C00000',
    'danger_light': '#FFC7CE',
    'danger_text': '#9C0006',
    'warning_dark': '#7F6000',
    'warning': '#FFC000',
    'warning_light': '#FFEB9C',
    'warning_text': '#9C5700',
    'white': '#FFFFFF',
    'light_gray': '#F2F2F2',
    'dark_gray': '#404040',
}

TABLE_STYLE = 'Table Style Medium 2'


def safe_float(value, default=None) -> Optional[float]:
    """Safely convert to float."""
    if value is None:
        return default
    try:
        return float(value)
    except (ValueError, TypeError):
        return default


class HistoryExporter:
    """Professional History Excel Exporter."""

    def __init__(self, workbook: xlsxwriter.Workbook, conn: sqlite3.Connection,
                 start_date: str = None, end_date: str = None, stock_id: str = None):
        self.wb = workbook
        self.conn = conn
        self.start_date = start_date
        self.end_date = end_date
        self.stock_id = stock_id
        self._setup_formats()

    def _setup_formats(self):
        """Setup formatting styles - matches export_dashboard.py."""
        self.fmt_title = self.wb.add_format({
            'bold': True, 'font_size': 18, 'font_color': COLORS['primary_dark'],
            'bottom': 2, 'bottom_color': COLORS['primary_dark']
        })
        self.fmt_subtitle = self.wb.add_format({
            'bold': True, 'font_size': 12, 'font_color': COLORS['primary']
        })
        self.fmt_section = self.wb.add_format({
            'bold': True, 'font_size': 14, 'font_color': COLORS['white'],
            'bg_color': COLORS['primary_dark'], 'align': 'center', 'valign': 'vcenter'
        })
        self.fmt_header = self.wb.add_format({
            'bold': True, 'font_color': COLORS['white'],
            'bg_color': COLORS['primary_dark'], 'align': 'center', 'valign': 'vcenter',
            'border': 1, 'text_wrap': True
        })
        self.fmt_num = self.wb.add_format({'num_format': '#,##0.00', 'align': 'right'})
        self.fmt_num_0 = self.wb.add_format({'num_format': '#,##0', 'align': 'right'})
        self.fmt_pct = self.wb.add_format({'num_format': '0.00"%"', 'align': 'right'})
        self.fmt_pct_signed = self.wb.add_format({'num_format': '+0.00%;-0.00%;0.00%', 'align': 'right'})
        self.fmt_date = self.wb.add_format({'num_format': 'yyyy-mm-dd', 'align': 'center'})
        self.fmt_kpi_value = self.wb.add_format({
            'bold': True, 'font_size': 24, 'font_color': COLORS['primary_dark'],
            'align': 'center', 'valign': 'vcenter'
        })
        self.fmt_kpi_label = self.wb.add_format({
            'font_size': 10, 'font_color': COLORS['dark_gray'],
            'align': 'center', 'valign': 'vcenter', 'bold': True
        })
        self.fmt_kpi_box = self.wb.add_format({
            'bg_color': COLORS['light_gray'], 'border': 1, 'border_color': COLORS['primary_light']
        })

    def _add_table(self, ws, start_row: int, start_col: int, data: list,
                   columns: list, table_name: str, total_row: bool = False) -> int:
        """Add a proper Excel Table with filtering and sorting."""
        if not data:
            ws.write(start_row, start_col, "No data available")
            return start_row + 1

        end_row = start_row + len(data)
        end_col = start_col + len(columns) - 1

        # Build table columns
        table_columns = []
        for col_def in columns:
            col_opt = {'header': col_def['header']}
            if col_def.get('total_function'):
                col_opt['total_function'] = col_def['total_function']
            if col_def.get('total_string'):
                col_opt['total_string'] = col_def['total_string']
            if col_def.get('format'):
                col_opt['format'] = col_def['format']
            table_columns.append(col_opt)

        # Write data
        for row_idx, row_data in enumerate(data):
            for col_idx, col_def in enumerate(columns):
                key = col_def.get('key')
                value = row_data.get(key) if key else None

                transform = col_def.get('transform')
                if transform and value is not None:
                    value = transform(value)

                fmt = col_def.get('format')
                ws.write(start_row + 1 + row_idx, start_col + col_idx, value, fmt)

        # Add table
        table_range = xl_range(start_row, start_col, end_row, end_col)
        ws.add_table(table_range, {
            'name': table_name,
            'style': TABLE_STYLE,
            'columns': table_columns,
            'total_row': total_row,
            'autofilter': True,
        })

        # Set column widths
        for col_idx, col_def in enumerate(columns):
            width = col_def.get('width', 12)
            ws.set_column(start_col + col_idx, start_col + col_idx, width)

        return end_row + (2 if total_row else 1)

    def _get_stocks(self) -> list:
        """Get all stocks."""
        cursor = self.conn.execute("""
            SELECT id, ticker, name, exchange_code as exchange
            FROM stocks ORDER BY ticker
        """)
        return [dict(row) for row in cursor.fetchall()]

    def _get_price_history(self) -> list:
        """Get price history with filters."""
        query = """
            SELECT ph.*, s.ticker, s.name
            FROM price_history ph
            JOIN stocks s ON ph.stock_id = s.id
            WHERE 1=1
        """
        params = []

        if self.stock_id:
            query += " AND ph.stock_id = ?"
            params.append(self.stock_id)
        if self.start_date:
            query += " AND ph.scrape_date >= ?"
            params.append(self.start_date)
        if self.end_date:
            query += " AND ph.scrape_date <= ?"
            params.append(self.end_date)

        query += " ORDER BY s.ticker, ph.scrape_date DESC"
        return [dict(row) for row in self.conn.execute(query, params).fetchall()]

    def _get_ratios_history(self) -> list:
        """Get ratios history with filters."""
        query = """
            SELECT rh.*, s.ticker, s.name
            FROM ratios_history rh
            JOIN stocks s ON rh.stock_id = s.id
            WHERE 1=1
        """
        params = []

        if self.stock_id:
            query += " AND rh.stock_id = ?"
            params.append(self.stock_id)
        if self.start_date:
            query += " AND rh.scrape_date >= ?"
            params.append(self.start_date)
        if self.end_date:
            query += " AND rh.scrape_date <= ?"
            params.append(self.end_date)

        query += " ORDER BY s.ticker, rh.scrape_date DESC"
        return [dict(row) for row in self.conn.execute(query, params).fetchall()]

    def _get_sentiment_history(self) -> list:
        """Get sentiment history with filters."""
        query = """
            SELECT sh.*, s.ticker, s.name
            FROM sentiment_history sh
            JOIN stocks s ON sh.stock_id = s.id
            WHERE 1=1
        """
        params = []

        if self.stock_id:
            query += " AND sh.stock_id = ?"
            params.append(self.stock_id)
        if self.start_date:
            query += " AND sh.scrape_date >= ?"
            params.append(self.start_date)
        if self.end_date:
            query += " AND sh.scrape_date <= ?"
            params.append(self.end_date)

        query += " ORDER BY s.ticker, sh.scrape_date DESC"
        return [dict(row) for row in self.conn.execute(query, params).fetchall()]

    def _get_scrape_runs(self) -> list:
        """Get scrape runs."""
        cursor = self.conn.execute("""
            SELECT * FROM scrape_runs ORDER BY started_at DESC
        """)
        return [dict(row) for row in cursor.fetchall()]

    def create_summary_sheet(self, stocks: list, price_history: list,
                             ratios_history: list, sentiment_history: list,
                             scrape_runs: list):
        """Create summary sheet."""
        ws = self.wb.add_worksheet("Summary")

        # Title
        ws.merge_range('A1:F1', "Historical Data Summary", self.fmt_title)
        ws.set_row(0, 30)
        ws.write('A2', f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}", self.fmt_subtitle)

        # KPI cards
        row = 4
        kpis = [
            ("Total Stocks", str(len(stocks))),
            ("Price Records", str(len(price_history))),
            ("Ratio Records", str(len(ratios_history))),
            ("Sentiment Records", str(len(sentiment_history))),
            ("Scrape Runs", str(len(scrape_runs))),
        ]

        col = 0
        for label, value in kpis:
            ws.merge_range(row, col, row + 1, col + 1, '', self.fmt_kpi_box)
            ws.write(row, col, label, self.fmt_kpi_label)
            ws.write(row + 1, col, value, self.fmt_kpi_value)
            col += 2

        # Date range
        row = 7
        ws.write(row, 0, "Data Date Range:", self.fmt_subtitle)

        if price_history:
            dates = [p['scrape_date'] for p in price_history if p.get('scrape_date')]
            if dates:
                ws.write(row + 1, 0, f"From: {min(dates)}")
                ws.write(row + 2, 0, f"To: {max(dates)}")

        # Scrape runs summary table
        row = 11
        ws.merge_range(row, 0, row, 5, "Recent Scrape Runs", self.fmt_section)
        row += 1

        run_data = []
        for r in scrape_runs[:10]:
            started = r.get('started_at', '')
            finished = r.get('finished_at', '')

            duration = ''
            if started and finished:
                try:
                    start_dt = datetime.fromisoformat(started.replace('Z', '+00:00'))
                    finish_dt = datetime.fromisoformat(finished.replace('Z', '+00:00'))
                    delta = finish_dt - start_dt
                    duration = str(delta)
                except:
                    pass

            run_data.append({
                'id': r.get('id'),
                'status': r.get('status'),
                'index': r.get('index_name'),
                'total': r.get('total_stocks'),
                'success': r.get('success'),
                'failed': r.get('failed'),
                'duration': duration,
            })

        run_cols = [
            {'header': 'Run ID', 'key': 'id', 'width': 8},
            {'header': 'Status', 'key': 'status', 'width': 12},
            {'header': 'Index', 'key': 'index', 'width': 10},
            {'header': 'Total', 'key': 'total', 'width': 8, 'format': self.fmt_num_0},
            {'header': 'Success', 'key': 'success', 'width': 8, 'format': self.fmt_num_0},
            {'header': 'Failed', 'key': 'failed', 'width': 8, 'format': self.fmt_num_0},
            {'header': 'Duration', 'key': 'duration', 'width': 15},
        ]

        self._add_table(ws, row, 0, run_data, run_cols, 'ScrapeRuns')

        ws.set_column('A:G', 12)

    def create_price_history_sheet(self, data: list):
        """Create price history sheet."""
        ws = self.wb.add_worksheet("Price History")

        ws.merge_range('A1:N1', "Price History", self.fmt_title)
        ws.set_row(0, 25)

        row = 3
        price_data = [{
            'ticker': p['ticker'],
            'name': p['name'],
            'date': p.get('scrape_date', ''),
            'price': safe_float(p.get('price')),
            'change': safe_float(p.get('price_change')),
            'change_pct': safe_float(p.get('price_change_pct'), 0) / 100 if p.get('price_change_pct') else None,
            'open': safe_float(p.get('price_open')),
            'high': safe_float(p.get('price_high')),
            'low': safe_float(p.get('price_low')),
            'volume': safe_float(p.get('volume')),
            'market_cap': safe_float(p.get('market_cap')),
            'high_52w': safe_float(p.get('price_52w_high')),
            'low_52w': safe_float(p.get('price_52w_low')),
            'ytd_pct': safe_float(p.get('price_change_ytd'), 0) / 100 if p.get('price_change_ytd') else None,
        } for p in data]

        price_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 22},
            {'header': 'Date', 'key': 'date', 'width': 11},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_num},
            {'header': 'Change', 'key': 'change', 'width': 10, 'format': self.fmt_num},
            {'header': 'Chg%', 'key': 'change_pct', 'width': 8, 'format': self.fmt_pct_signed},
            {'header': 'Open', 'key': 'open', 'width': 10, 'format': self.fmt_num},
            {'header': 'High', 'key': 'high', 'width': 10, 'format': self.fmt_num},
            {'header': 'Low', 'key': 'low', 'width': 10, 'format': self.fmt_num},
            {'header': 'Volume', 'key': 'volume', 'width': 14, 'format': self.fmt_num_0},
            {'header': 'Market Cap', 'key': 'market_cap', 'width': 15, 'format': self.fmt_num_0},
            {'header': '52W High', 'key': 'high_52w', 'width': 10, 'format': self.fmt_num},
            {'header': '52W Low', 'key': 'low_52w', 'width': 10, 'format': self.fmt_num},
            {'header': 'YTD%', 'key': 'ytd_pct', 'width': 8, 'format': self.fmt_pct_signed},
        ]

        self._add_table(ws, row, 0, price_data, price_cols, 'PriceHistory')

        # Conditional formatting on change %
        if price_data:
            data_end = row + len(price_data)
            ws.conditional_format(row + 1, 5, data_end, 5, {
                'type': '3_color_scale',
                'min_color': '#F8696B',
                'mid_color': '#FFEB84',
                'max_color': '#63BE7B',
            })

        ws.freeze_panes(4, 2)

    def create_ratios_history_sheet(self, data: list):
        """Create ratios history sheet."""
        ws = self.wb.add_worksheet("Ratios History")

        ws.merge_range('A1:W1', "Financial Ratios History", self.fmt_title)
        ws.set_row(0, 25)

        row = 3
        ratio_data = [{
            'ticker': r['ticker'],
            'name': r['name'],
            'date': r.get('scrape_date', ''),
            'year': r.get('year', ''),
            'pe': safe_float(r.get('pe_ratio')),
            'pb': safe_float(r.get('pb_ratio')),
            'ps': safe_float(r.get('ps_ratio')),
            'pcf': safe_float(r.get('pcf_ratio')),
            'ev_ebitda': safe_float(r.get('ev_ebitda')),
            'roe': safe_float(r.get('roe')),
            'roa': safe_float(r.get('roa')),
            'roic': safe_float(r.get('roic')),
            'gross': safe_float(r.get('gross_margin')),
            'op_margin': safe_float(r.get('operating_margin')),
            'net_margin': safe_float(r.get('net_margin')),
            'de': safe_float(r.get('debt_to_equity')),
            'current': safe_float(r.get('current_ratio')),
            'quick': safe_float(r.get('quick_ratio')),
            'div_yield': safe_float(r.get('dividend_yield')),
            'payout': safe_float(r.get('payout_ratio')),
            'eps': safe_float(r.get('eps')),
            'bvps': safe_float(r.get('bvps')),
            'rev_gr': safe_float(r.get('revenue_growth')),
        } for r in data]

        ratio_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 20},
            {'header': 'Date', 'key': 'date', 'width': 11},
            {'header': 'Year', 'key': 'year', 'width': 6},
            {'header': 'P/E', 'key': 'pe', 'width': 7, 'format': self.fmt_num},
            {'header': 'P/B', 'key': 'pb', 'width': 7, 'format': self.fmt_num},
            {'header': 'P/S', 'key': 'ps', 'width': 7, 'format': self.fmt_num},
            {'header': 'P/CF', 'key': 'pcf', 'width': 7, 'format': self.fmt_num},
            {'header': 'EV/EBITDA', 'key': 'ev_ebitda', 'width': 9, 'format': self.fmt_num},
            {'header': 'ROE%', 'key': 'roe', 'width': 7, 'format': self.fmt_num},
            {'header': 'ROA%', 'key': 'roa', 'width': 7, 'format': self.fmt_num},
            {'header': 'ROIC%', 'key': 'roic', 'width': 7, 'format': self.fmt_num},
            {'header': 'Gross%', 'key': 'gross', 'width': 8, 'format': self.fmt_num},
            {'header': 'Op%', 'key': 'op_margin', 'width': 7, 'format': self.fmt_num},
            {'header': 'Net%', 'key': 'net_margin', 'width': 7, 'format': self.fmt_num},
            {'header': 'D/E', 'key': 'de', 'width': 7, 'format': self.fmt_num},
            {'header': 'Current', 'key': 'current', 'width': 8, 'format': self.fmt_num},
            {'header': 'Quick', 'key': 'quick', 'width': 7, 'format': self.fmt_num},
            {'header': 'Yield%', 'key': 'div_yield', 'width': 7, 'format': self.fmt_num},
            {'header': 'Payout%', 'key': 'payout', 'width': 8, 'format': self.fmt_num},
            {'header': 'EPS', 'key': 'eps', 'width': 8, 'format': self.fmt_num},
            {'header': 'BVPS', 'key': 'bvps', 'width': 9, 'format': self.fmt_num},
            {'header': 'RevGr%', 'key': 'rev_gr', 'width': 8, 'format': self.fmt_num},
        ]

        self._add_table(ws, row, 0, ratio_data, ratio_cols, 'RatiosHistory')

        # Conditional formatting on ROE
        if ratio_data:
            data_end = row + len(ratio_data)
            ws.conditional_format(row + 1, 9, data_end, 9, {
                'type': 'data_bar',
                'bar_color': COLORS['success'],
                'bar_solid': True,
            })

        ws.freeze_panes(4, 2)

    def create_sentiment_history_sheet(self, data: list):
        """Create sentiment history sheet."""
        ws = self.wb.add_worksheet("Sentiment History")

        ws.merge_range('A1:K1', "Sentiment History", self.fmt_title)
        ws.set_row(0, 25)

        row = 3
        sent_data = [{
            'ticker': s['ticker'],
            'name': s['name'],
            'date': s.get('scrape_date', ''),
            'time_range': s.get('time_range', ''),
            'bullish': safe_float(s.get('bullish_pct')),
            'bearish': safe_float(s.get('bearish_pct')),
            'neutral': safe_float(s.get('neutral_pct')),
            'bull_count': safe_float(s.get('bullish')),
            'bear_count': safe_float(s.get('bearish')),
            'neut_count': safe_float(s.get('neutral')),
            'net': (safe_float(s.get('bullish_pct'), 0) - safe_float(s.get('bearish_pct'), 0)),
        } for s in data]

        sent_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 22},
            {'header': 'Date', 'key': 'date', 'width': 11},
            {'header': 'Period', 'key': 'time_range', 'width': 15},
            {'header': 'Bull%', 'key': 'bullish', 'width': 8, 'format': self.fmt_num},
            {'header': 'Bear%', 'key': 'bearish', 'width': 8, 'format': self.fmt_num},
            {'header': 'Neut%', 'key': 'neutral', 'width': 8, 'format': self.fmt_num},
            {'header': 'Bulls', 'key': 'bull_count', 'width': 8, 'format': self.fmt_num_0},
            {'header': 'Bears', 'key': 'bear_count', 'width': 8, 'format': self.fmt_num_0},
            {'header': 'Neutral', 'key': 'neut_count', 'width': 8, 'format': self.fmt_num_0},
            {'header': 'Net', 'key': 'net', 'width': 8, 'format': self.fmt_num},
        ]

        self._add_table(ws, row, 0, sent_data, sent_cols, 'SentimentHistory')

        # Conditional formatting
        if sent_data:
            data_end = row + len(sent_data)
            # Bullish data bar
            ws.conditional_format(row + 1, 4, data_end, 4, {
                'type': 'data_bar',
                'bar_color': COLORS['success'],
                'bar_solid': True,
            })
            # Bearish data bar
            ws.conditional_format(row + 1, 5, data_end, 5, {
                'type': 'data_bar',
                'bar_color': COLORS['danger'],
                'bar_solid': True,
            })
            # Net sentiment 3-color
            ws.conditional_format(row + 1, 10, data_end, 10, {
                'type': '3_color_scale',
                'min_color': '#F8696B',
                'mid_color': '#FFEB84',
                'max_color': '#63BE7B',
            })

        ws.freeze_panes(4, 2)

    def create_price_pivot_sheet(self, stocks: list, price_history: list):
        """Create price pivot table (dates as rows, stocks as columns)."""
        ws = self.wb.add_worksheet("Price Pivot")

        ws.merge_range('A1:C1', "Price Matrix (Pivot)", self.fmt_title)
        ws.set_row(0, 25)

        if not price_history or not stocks:
            ws.write(3, 0, "No data available")
            return

        # Get unique dates (limit to 365)
        dates = sorted(set(p['scrape_date'] for p in price_history if p.get('scrape_date')),
                       reverse=True)[:365]

        if not dates:
            ws.write(3, 0, "No date data available")
            return

        # Limit to 100 stocks
        stock_list = stocks[:100]

        # Build price lookup
        price_lookup = {}
        for p in price_history:
            key = (p['stock_id'], p['scrape_date'])
            price_lookup[key] = p.get('price')

        # Header row with tickers
        row = 3
        ws.write(row, 0, "Date", self.fmt_header)
        for col, stock in enumerate(stock_list, 1):
            ws.write(row, col, stock['ticker'], self.fmt_header)

        # Data rows
        for row_idx, date in enumerate(dates):
            ws.write(row + 1 + row_idx, 0, date)
            for col_idx, stock in enumerate(stock_list, 1):
                price = price_lookup.get((stock['id'], date))
                if price:
                    ws.write(row + 1 + row_idx, col_idx, price, self.fmt_num)

        ws.freeze_panes(4, 1)
        ws.set_column(0, 0, 12)
        ws.set_column(1, 100, 10)

    def generate(self):
        """Generate all history sheets."""
        print("Loading data...")
        stocks = self._get_stocks()
        price_history = self._get_price_history()
        ratios_history = self._get_ratios_history()
        sentiment_history = self._get_sentiment_history()
        scrape_runs = self._get_scrape_runs()

        print("Creating Summary sheet...")
        self.create_summary_sheet(stocks, price_history, ratios_history,
                                  sentiment_history, scrape_runs)

        print("Creating Price History sheet...")
        self.create_price_history_sheet(price_history)

        print("Creating Ratios History sheet...")
        self.create_ratios_history_sheet(ratios_history)

        print("Creating Sentiment History sheet...")
        self.create_sentiment_history_sheet(sentiment_history)

        if stocks and price_history:
            print("Creating Price Pivot sheet...")
            self.create_price_pivot_sheet(stocks, price_history)


def main():
    parser = argparse.ArgumentParser(
        description="Export historical stock data to professional Excel"
    )
    parser.add_argument('--db', required=True, help='Path to SQLite database')
    parser.add_argument('--output', '-o', required=True, help='Output Excel file path')
    parser.add_argument('--start-date', help='Start date filter (YYYY-MM-DD)')
    parser.add_argument('--end-date', help='End date filter (YYYY-MM-DD)')
    parser.add_argument('--stock', help='Filter by stock ID')

    args = parser.parse_args()

    if not Path(args.db).exists():
        print(f"Error: Database not found: {args.db}")
        return 1

    conn = sqlite3.connect(args.db)
    conn.row_factory = sqlite3.Row

    workbook = xlsxwriter.Workbook(args.output, {
        'constant_memory': False,
        'strings_to_urls': True,
    })

    exporter = HistoryExporter(
        workbook, conn,
        start_date=args.start_date,
        end_date=args.end_date,
        stock_id=args.stock
    )
    exporter.generate()

    workbook.close()
    conn.close()

    print(f"\nHistory exported to: {args.output}")
    return 0


if __name__ == "__main__":
    exit(main())
