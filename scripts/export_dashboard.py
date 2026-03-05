#!/usr/bin/env python3
"""
Professional IDX Stock Dashboard - Excel Export

Creates professional Excel dashboard with:
- Clean Excel Tables with AutoFilter on every column
- Proper number formatting (currency, percentages, ratios)
- Conditional formatting (color scales, data bars)
- Professional styling (consistent headers, colors)
- NO CHARTS - tables only for clean, data-focused look

Usage:
    uv run python scripts/export_dashboard.py --db output/stocks.db --output output/dashboard.xlsx
"""

import argparse
import sqlite3
import math
from datetime import datetime
from pathlib import Path
from typing import Optional, List, Dict, Any

import xlsxwriter
from xlsxwriter.utility import xl_range

# ============================================================================
# Color Palette - Professional Finance Theme
# ============================================================================

COLORS = {
    # Primary blues
    'primary_dark': '#1F4E79',
    'primary': '#2E75B6',
    'primary_light': '#5B9BD5',
    'primary_bg': '#D6DCE4',

    # Success greens
    'success_dark': '#375623',
    'success': '#70AD47',
    'success_light': '#C6EFCE',
    'success_text': '#006100',

    # Danger reds
    'danger_dark': '#833C0C',
    'danger': '#C00000',
    'danger_light': '#FFC7CE',
    'danger_text': '#9C0006',

    # Warning yellows
    'warning_dark': '#7F6000',
    'warning': '#FFC000',
    'warning_light': '#FFEB9C',
    'warning_text': '#9C5700',

    # Neutral
    'white': '#FFFFFF',
    'light_gray': '#F2F2F2',
    'mid_gray': '#D9D9D9',
    'dark_gray': '#404040',
    'black': '#000000',
}

# Table styles (built-in Excel styles)
TABLE_STYLE_BLUE = 'Table Style Medium 2'
TABLE_STYLE_GREEN = 'Table Style Medium 7'
TABLE_STYLE_RED = 'Table Style Medium 3'
TABLE_STYLE_ORANGE = 'Table Style Medium 4'
TABLE_STYLE_DARK = 'Table Style Dark 1'


# ============================================================================
# Utility Functions
# ============================================================================

def safe_float(value, default=None) -> Optional[float]:
    """Safely convert value to float."""
    if value is None:
        return default
    try:
        return float(value)
    except (ValueError, TypeError):
        return default


def calc_graham_number(eps: float, bvps: float) -> Optional[float]:
    """Calculate Graham Number = sqrt(22.5 * EPS * BVPS)."""
    if eps is None or bvps is None or eps <= 0 or bvps <= 0:
        return None
    return math.sqrt(22.5 * eps * bvps)


def calc_graham_margin(price: float, graham: float) -> Optional[float]:
    """Calculate margin of safety vs Graham Number as percentage."""
    if price is None or graham is None or price == 0:
        return None
    return ((graham - price) / price) * 100


def calc_52w_position(price: float, high: float, low: float) -> Optional[float]:
    """Calculate position within 52-week range (0-100)."""
    if price is None or high is None or low is None:
        return None
    if high == low:
        return 50.0
    return ((price - low) / (high - low)) * 100


def calc_health_score(data: dict) -> int:
    """Calculate financial health score (0-8 points)."""
    score = 0
    if safe_float(data.get('roe'), 0) > 10:
        score += 1
    if safe_float(data.get('roa'), 0) > 5:
        score += 1
    if safe_float(data.get('net_margin'), 0) > 0:
        score += 1
    if safe_float(data.get('operating_margin'), 0) > 0:
        score += 1
    de = safe_float(data.get('debt_to_equity'))
    if de is not None and de < 1:
        score += 1
    cr = safe_float(data.get('current_ratio'))
    if cr is not None and cr > 1:
        score += 1
    if safe_float(data.get('revenue_growth'), 0) > 0:
        score += 1
    if safe_float(data.get('earnings_growth'), 0) > 0:
        score += 1
    return score


def calc_dividend_safety(payout: float, de_ratio: float) -> str:
    """Rate dividend safety based on payout ratio and leverage."""
    if payout is None:
        return "N/A"
    score = 0
    if payout < 50:
        score += 3
    elif payout < 70:
        score += 2
    elif payout < 90:
        score += 1
    if de_ratio is not None:
        if de_ratio < 0.5:
            score += 2
        elif de_ratio < 1.0:
            score += 1
    if score >= 4:
        return "SAFE"
    elif score >= 2:
        return "OK"
    return "RISKY"


# ============================================================================
# Dashboard Generator
# ============================================================================

class ProfessionalDashboard:
    """Professional Excel Dashboard Generator using xlsxwriter."""

    def __init__(self, workbook: xlsxwriter.Workbook, conn: sqlite3.Connection, date: str):
        self.wb = workbook
        self.conn = conn
        self.date = date
        self._setup_formats()
        self._load_data()

    def _setup_formats(self):
        """Setup all formatting styles for consistent look."""

        # ===== TITLE & HEADER FORMATS =====
        self.fmt_title = self.wb.add_format({
            'bold': True,
            'font_size': 18,
            'font_color': COLORS['primary_dark'],
            'bottom': 2,
            'bottom_color': COLORS['primary_dark'],
        })

        self.fmt_subtitle = self.wb.add_format({
            'font_size': 10,
            'font_color': COLORS['dark_gray'],
            'italic': True,
        })

        # Section headers with solid background
        self.fmt_section = self.wb.add_format({
            'bold': True,
            'font_size': 14,
            'font_color': COLORS['white'],
            'bg_color': COLORS['primary_dark'],
            'align': 'center',
            'valign': 'vcenter',
            'border': 1,
            'border_color': COLORS['primary_dark'],
        })

        self.fmt_section_green = self.wb.add_format({
            'bold': True,
            'font_size': 14,
            'font_color': COLORS['white'],
            'bg_color': COLORS['success_dark'],
            'align': 'center',
            'valign': 'vcenter',
            'border': 1,
        })

        self.fmt_section_red = self.wb.add_format({
            'bold': True,
            'font_size': 14,
            'font_color': COLORS['white'],
            'bg_color': COLORS['danger_dark'],
            'align': 'center',
            'valign': 'vcenter',
            'border': 1,
        })

        self.fmt_section_orange = self.wb.add_format({
            'bold': True,
            'font_size': 14,
            'font_color': COLORS['white'],
            'bg_color': COLORS['warning_dark'],
            'align': 'center',
            'valign': 'vcenter',
            'border': 1,
        })

        # ===== NUMBER FORMATS =====
        # Integer (no decimals)
        self.fmt_int = self.wb.add_format({
            'num_format': '#,##0',
            'align': 'right',
        })

        # Two decimal places
        self.fmt_dec2 = self.wb.add_format({
            'num_format': '#,##0.00',
            'align': 'right',
        })

        # One decimal place
        self.fmt_dec1 = self.wb.add_format({
            'num_format': '#,##0.0',
            'align': 'right',
        })

        # Percentage with sign (for change values stored as decimal like 0.05 = 5%)
        self.fmt_pct_sign = self.wb.add_format({
            'num_format': '+0.00%;-0.00%;0.00%',
            'align': 'right',
        })

        # Percentage from whole number (for values stored as 5.0 = 5%)
        self.fmt_pct = self.wb.add_format({
            'num_format': '0.00"%"',
            'align': 'right',
        })

        self.fmt_pct1 = self.wb.add_format({
            'num_format': '0.0"%"',
            'align': 'right',
        })

        # Ratio (2 decimal places, no suffix)
        self.fmt_ratio = self.wb.add_format({
            'num_format': '0.00',
            'align': 'right',
        })

        # Currency Rupiah
        self.fmt_currency = self.wb.add_format({
            'num_format': '"Rp "#,##0',
            'align': 'right',
        })

        # Billions
        self.fmt_billions = self.wb.add_format({
            'num_format': '#,##0.00"B"',
            'align': 'right',
        })

        # Trillions
        self.fmt_trillions = self.wb.add_format({
            'num_format': '#,##0.00"T"',
            'align': 'right',
        })

        # Score (centered integer)
        self.fmt_score = self.wb.add_format({
            'num_format': '0',
            'align': 'center',
            'bold': True,
        })

        # ===== STATUS FORMATS =====
        self.fmt_good = self.wb.add_format({
            'bg_color': COLORS['success_light'],
            'font_color': COLORS['success_text'],
            'bold': True,
            'align': 'center',
            'border': 1,
            'border_color': COLORS['success'],
        })

        self.fmt_bad = self.wb.add_format({
            'bg_color': COLORS['danger_light'],
            'font_color': COLORS['danger_text'],
            'bold': True,
            'align': 'center',
            'border': 1,
            'border_color': COLORS['danger'],
        })

        self.fmt_warn = self.wb.add_format({
            'bg_color': COLORS['warning_light'],
            'font_color': COLORS['warning_text'],
            'bold': True,
            'align': 'center',
            'border': 1,
            'border_color': COLORS['warning'],
        })

        self.fmt_neutral = self.wb.add_format({
            'bg_color': COLORS['light_gray'],
            'align': 'center',
        })

        # ===== KPI BOX FORMATS =====
        self.fmt_kpi_value = self.wb.add_format({
            'bold': True,
            'font_size': 24,
            'font_color': COLORS['primary_dark'],
            'align': 'center',
            'valign': 'vcenter',
        })

        self.fmt_kpi_label = self.wb.add_format({
            'font_size': 10,
            'font_color': COLORS['dark_gray'],
            'align': 'center',
            'valign': 'vcenter',
            'bold': True,
        })

    def _load_data(self):
        """Load and enrich stock data from database."""
        query = """
        SELECT
            s.id, s.ticker, s.name, s.sector, s.industry,
            p.price, p.price_change, p.price_change_pct,
            p.price_52w_high, p.price_52w_low,
            p.volume, p.avg_volume, p.market_cap,
            p.return_1w, p.return_1m, p.return_3m, p.return_6m, p.return_ytd, p.return_1y,
            r.pe_ratio, r.pb_ratio, r.ps_ratio, r.ev_ebitda,
            r.dividend_yield, r.payout_ratio, r.roe, r.roa, r.roic,
            r.gross_margin, r.operating_margin, r.net_margin,
            r.debt_to_equity, r.current_ratio, r.quick_ratio,
            r.revenue_growth, r.earnings_growth, r.eps, r.bvps,
            sh.bullish_pct, sh.bearish_pct, sh.neutral_pct
        FROM stocks s
        LEFT JOIN price_history p ON s.id = p.stock_id AND p.scrape_date = ?
        LEFT JOIN (
            SELECT stock_id, pe_ratio, pb_ratio, ps_ratio, ev_ebitda,
                   dividend_yield, payout_ratio, roe, roa, roic,
                   gross_margin, operating_margin, net_margin,
                   debt_to_equity, current_ratio, quick_ratio,
                   revenue_growth, earnings_growth, eps, bvps
            FROM ratios_history WHERE scrape_date = ?
            GROUP BY stock_id HAVING MAX(year)
        ) r ON s.id = r.stock_id
        LEFT JOIN (
            SELECT stock_id, bullish_pct, bearish_pct, neutral_pct
            FROM sentiment_history WHERE scrape_date = ? AND time_range_enum = 'week'
        ) sh ON s.id = sh.stock_id
        WHERE p.price IS NOT NULL
        ORDER BY p.market_cap DESC NULLS LAST
        """
        rows = self.conn.execute(query, (self.date, self.date, self.date)).fetchall()
        self.stocks = [dict(row) for row in rows]

        # Enrich with calculated fields
        for s in self.stocks:
            # Market cap conversions
            mcap = safe_float(s.get('market_cap'), 0)
            s['market_cap_b'] = mcap / 1e9
            s['market_cap_t'] = mcap / 1e12

            # Graham number & margin
            s['graham_num'] = calc_graham_number(
                safe_float(s.get('eps')), safe_float(s.get('bvps')))
            s['graham_margin'] = calc_graham_margin(
                safe_float(s.get('price')), s['graham_num'])

            # 52-week position
            s['pos_52w'] = calc_52w_position(
                safe_float(s.get('price')),
                safe_float(s.get('price_52w_high')),
                safe_float(s.get('price_52w_low')))

            # Health score
            s['health_score'] = calc_health_score(s)

            # Convert price_change_pct to decimal for proper % formatting
            pct = safe_float(s.get('price_change_pct'))
            s['price_change_pct_dec'] = pct / 100 if pct is not None else None

    def _add_table(self, ws, start_row: int, start_col: int, data: list,
                   columns: list, table_name: str, style: str = TABLE_STYLE_BLUE,
                   total_row: bool = False) -> int:
        """
        Add Excel Table with proper formatting and AutoFilter.
        Returns the row number after the table.
        """
        if not data:
            ws.write(start_row, start_col, "No data available", self.fmt_neutral)
            return start_row + 1

        end_row = start_row + len(data)
        end_col = start_col + len(columns) - 1

        # Build table column configuration
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

        # Write data cells with formatting
        for row_idx, row_data in enumerate(data):
            for col_idx, col_def in enumerate(columns):
                key = col_def.get('key')
                value = row_data.get(key) if key else None

                # Apply transform if specified
                transform = col_def.get('transform')
                if transform and value is not None:
                    value = transform(value)

                cell_fmt = col_def.get('format')
                ws.write(start_row + 1 + row_idx, start_col + col_idx, value, cell_fmt)

        # Create the table
        table_range = xl_range(start_row, start_col, end_row, end_col)
        ws.add_table(table_range, {
            'name': table_name,
            'style': style,
            'columns': table_columns,
            'total_row': total_row,
            'autofilter': True,
        })

        # Set column widths
        for col_idx, col_def in enumerate(columns):
            width = col_def.get('width', 12)
            ws.set_column(start_col + col_idx, start_col + col_idx, width)

        return end_row + (2 if total_row else 1)

    def _add_cond_fmt(self, ws, start_row: int, end_row: int, col: int,
                      fmt_type: str, **kwargs):
        """Add conditional formatting to a column range."""
        cell_range = xl_range(start_row, col, end_row, col)

        if fmt_type == '3_color_scale':
            ws.conditional_format(cell_range, {
                'type': '3_color_scale',
                'min_color': kwargs.get('min_color', '#F8696B'),
                'mid_color': kwargs.get('mid_color', '#FFEB84'),
                'max_color': kwargs.get('max_color', '#63BE7B'),
            })
        elif fmt_type == '2_color_scale':
            ws.conditional_format(cell_range, {
                'type': '2_color_scale',
                'min_color': kwargs.get('min_color', '#FFFFFF'),
                'max_color': kwargs.get('max_color', '#63BE7B'),
            })
        elif fmt_type == 'data_bar':
            ws.conditional_format(cell_range, {
                'type': 'data_bar',
                'bar_color': kwargs.get('bar_color', COLORS['primary_light']),
                'bar_solid': True,
            })
        elif fmt_type == 'icon_set':
            ws.conditional_format(cell_range, {
                'type': 'icon_set',
                'icon_style': kwargs.get('icon_style', '3_arrows'),
            })
        elif fmt_type == 'pos_neg':
            # Green for positive, red for negative
            ws.conditional_format(cell_range, {
                'type': 'cell',
                'criteria': '>',
                'value': 0,
                'format': self.wb.add_format({
                    'bg_color': COLORS['success_light'],
                    'font_color': COLORS['success_text'],
                }),
            })
            ws.conditional_format(cell_range, {
                'type': 'cell',
                'criteria': '<',
                'value': 0,
                'format': self.wb.add_format({
                    'bg_color': COLORS['danger_light'],
                    'font_color': COLORS['danger_text'],
                }),
            })

    # =========================================================================
    # SHEET: Executive Summary
    # =========================================================================
    def create_summary(self):
        """Create Executive Summary sheet with KPIs and overview tables."""
        ws = self.wb.add_worksheet("Executive Summary")
        ws.set_zoom(90)

        # Title
        ws.merge_range('A1:L1', f"IDX Market Dashboard - {self.date}", self.fmt_title)
        ws.set_row(0, 30)
        ws.write('A2', f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')} | Data Date: {self.date}", self.fmt_subtitle)

        # Calculate KPIs
        total = len(self.stocks)
        gainers = sum(1 for s in self.stocks if safe_float(s.get('price_change_pct'), 0) > 0)
        losers = sum(1 for s in self.stocks if safe_float(s.get('price_change_pct'), 0) < 0)
        total_mcap = sum(safe_float(s.get('market_cap'), 0) for s in self.stocks)

        pe_vals = [s['pe_ratio'] for s in self.stocks if s.get('pe_ratio') and 0 < s['pe_ratio'] < 100]
        avg_pe = sum(pe_vals) / len(pe_vals) if pe_vals else 0

        div_vals = [s['dividend_yield'] for s in self.stocks if s.get('dividend_yield') and s['dividend_yield'] > 0]
        avg_div = sum(div_vals) / len(div_vals) if div_vals else 0

        # KPI row
        kpis = [
            ('Total Stocks', str(total)),
            ('Market Cap', f"{total_mcap/1e12:.1f}T"),
            ('Gainers', str(gainers)),
            ('Losers', str(losers)),
            ('Avg P/E', f"{avg_pe:.1f}"),
            ('Avg Yield', f"{avg_div:.2f}%"),
        ]

        row = 4
        for col_idx, (label, value) in enumerate(kpis):
            ws.write(row, col_idx * 2, label, self.fmt_kpi_label)
            ws.write(row + 1, col_idx * 2, value, self.fmt_kpi_value)
            ws.set_column(col_idx * 2, col_idx * 2, 12)

        # Market Overview Table
        row = 8
        ws.merge_range(row, 0, row, 4, "MARKET OVERVIEW", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        overview_data = [
            {'metric': 'Total Stocks', 'value': total},
            {'metric': 'Total Market Cap (T)', 'value': total_mcap / 1e12},
            {'metric': 'Gainers', 'value': gainers},
            {'metric': 'Losers', 'value': losers},
            {'metric': 'Unchanged', 'value': total - gainers - losers},
            {'metric': 'Avg P/E Ratio', 'value': avg_pe},
            {'metric': 'Avg Div Yield %', 'value': avg_div},
        ]

        overview_cols = [
            {'header': 'Metric', 'key': 'metric', 'width': 20},
            {'header': 'Value', 'key': 'value', 'width': 15, 'format': self.fmt_dec2},
        ]

        row = self._add_table(ws, row, 0, overview_data, overview_cols, 'MarketOverview')

        # Top Gainers
        row += 1
        ws.merge_range(row, 0, row, 5, "TOP 15 GAINERS", self.fmt_section_green)
        ws.set_row(row, 22)
        row += 1

        top_gainers = sorted(
            [s for s in self.stocks if s.get('price_change_pct')],
            key=lambda x: safe_float(x['price_change_pct'], 0), reverse=True
        )[:15]

        gainer_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 22},
            {'header': 'Sector', 'key': 'sector', 'width': 16},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': 'Change', 'key': 'price_change', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Chg%', 'key': 'price_change_pct_dec', 'width': 10, 'format': self.fmt_pct_sign},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, top_gainers, gainer_cols, 'TopGainers', TABLE_STYLE_GREEN)
        self._add_cond_fmt(ws, data_start, end_row - 1, 5, 'data_bar', bar_color=COLORS['success'])

        # Top Losers
        row = end_row + 1
        ws.merge_range(row, 0, row, 5, "TOP 15 LOSERS", self.fmt_section_red)
        ws.set_row(row, 22)
        row += 1

        top_losers = sorted(
            [s for s in self.stocks if s.get('price_change_pct')],
            key=lambda x: safe_float(x['price_change_pct'], 0)
        )[:15]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, top_losers, gainer_cols, 'TopLosers', TABLE_STYLE_RED)
        self._add_cond_fmt(ws, data_start, end_row - 1, 5, 'data_bar', bar_color=COLORS['danger'])

        # Sector Breakdown (right side)
        sector_row = 8
        ws.merge_range(sector_row, 7, sector_row, 11, "SECTOR BREAKDOWN", self.fmt_section)
        ws.set_row(sector_row, 22)
        sector_row += 1

        sector_agg = {}
        for s in self.stocks:
            sector = s.get('sector') or 'Unknown'
            if sector not in sector_agg:
                sector_agg[sector] = {'count': 0, 'mcap': 0, 'gainers': 0, 'losers': 0}
            sector_agg[sector]['count'] += 1
            sector_agg[sector]['mcap'] += safe_float(s.get('market_cap'), 0)
            if safe_float(s.get('price_change_pct'), 0) > 0:
                sector_agg[sector]['gainers'] += 1
            elif safe_float(s.get('price_change_pct'), 0) < 0:
                sector_agg[sector]['losers'] += 1

        sector_data = [
            {'sector': k, 'count': v['count'], 'mcap': v['mcap'] / 1e12,
             'gainers': v['gainers'], 'losers': v['losers']}
            for k, v in sorted(sector_agg.items(), key=lambda x: x[1]['mcap'], reverse=True)
        ]

        sector_cols = [
            {'header': 'Sector', 'key': 'sector', 'width': 18},
            {'header': 'Stocks', 'key': 'count', 'width': 8, 'format': self.fmt_int, 'total_function': 'sum'},
            {'header': 'MCap(T)', 'key': 'mcap', 'width': 10, 'format': self.fmt_dec2, 'total_function': 'sum'},
            {'header': 'Gainers', 'key': 'gainers', 'width': 9, 'format': self.fmt_int, 'total_function': 'sum'},
            {'header': 'Losers', 'key': 'losers', 'width': 9, 'format': self.fmt_int, 'total_function': 'sum'},
        ]

        self._add_table(ws, sector_row, 7, sector_data, sector_cols, 'SectorBreakdown', total_row=True)

        ws.freeze_panes(3, 0)

    # =========================================================================
    # SHEET: All Stocks
    # =========================================================================
    def create_all_stocks(self):
        """Create comprehensive All Stocks data sheet."""
        ws = self.wb.add_worksheet("All Stocks")
        ws.set_zoom(85)

        columns = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 22},
            {'header': 'Sector', 'key': 'sector', 'width': 16},
            {'header': 'Industry', 'key': 'industry', 'width': 18},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': 'Chg', 'key': 'price_change', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'Chg%', 'key': 'price_change_pct_dec', 'width': 9, 'format': self.fmt_pct_sign},
            {'header': 'MCap(B)', 'key': 'market_cap_b', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Volume', 'key': 'volume', 'width': 12, 'format': self.fmt_int},
            {'header': 'P/E', 'key': 'pe_ratio', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'P/B', 'key': 'pb_ratio', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'P/S', 'key': 'ps_ratio', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'Div%', 'key': 'dividend_yield', 'width': 7, 'format': self.fmt_pct},
            {'header': 'ROE%', 'key': 'roe', 'width': 8, 'format': self.fmt_pct},
            {'header': 'ROA%', 'key': 'roa', 'width': 8, 'format': self.fmt_pct},
            {'header': 'Net%', 'key': 'net_margin', 'width': 8, 'format': self.fmt_pct},
            {'header': 'D/E', 'key': 'debt_to_equity', 'width': 7, 'format': self.fmt_dec2},
            {'header': 'Current', 'key': 'current_ratio', 'width': 8, 'format': self.fmt_dec2},
            {'header': '52H', 'key': 'price_52w_high', 'width': 10, 'format': self.fmt_int},
            {'header': '52L', 'key': 'price_52w_low', 'width': 10, 'format': self.fmt_int},
            {'header': '52W%', 'key': 'pos_52w', 'width': 8, 'format': self.fmt_pct1},
            {'header': '1W%', 'key': 'return_1w', 'width': 8, 'format': self.fmt_pct},
            {'header': '1M%', 'key': 'return_1m', 'width': 8, 'format': self.fmt_pct},
            {'header': 'YTD%', 'key': 'return_ytd', 'width': 8, 'format': self.fmt_pct},
            {'header': '1Y%', 'key': 'return_1y', 'width': 8, 'format': self.fmt_pct},
            {'header': 'Graham', 'key': 'graham_num', 'width': 10, 'format': self.fmt_int},
            {'header': 'GrhMgn%', 'key': 'graham_margin', 'width': 10, 'format': self.fmt_pct1},
            {'header': 'Score', 'key': 'health_score', 'width': 7, 'format': self.fmt_score},
        ]

        end_row = self._add_table(ws, 0, 0, self.stocks, columns, 'AllStocksData')

        # Conditional formatting
        n = len(self.stocks)
        self._add_cond_fmt(ws, 2, n + 1, 6, '3_color_scale')  # Chg%
        self._add_cond_fmt(ws, 2, n + 1, 13, 'data_bar', bar_color=COLORS['success'])  # ROE
        self._add_cond_fmt(ws, 2, n + 1, 16, '3_color_scale',
                          min_color='#63BE7B', mid_color='#FFEB84', max_color='#F8696B')  # D/E reversed
        self._add_cond_fmt(ws, 2, n + 1, 20, 'data_bar', bar_color=COLORS['primary_light'])  # 52W%
        self._add_cond_fmt(ws, 2, n + 1, 21, 'pos_neg')  # 1W%
        self._add_cond_fmt(ws, 2, n + 1, 22, 'pos_neg')  # 1M%
        self._add_cond_fmt(ws, 2, n + 1, 23, 'pos_neg')  # YTD%
        self._add_cond_fmt(ws, 2, n + 1, 24, 'pos_neg')  # 1Y%
        self._add_cond_fmt(ws, 2, n + 1, 26, '3_color_scale')  # Graham Margin
        self._add_cond_fmt(ws, 2, n + 1, 27, 'icon_set', icon_style='3_arrows')  # Score

        ws.freeze_panes(1, 2)

    # =========================================================================
    # SHEET: Valuation
    # =========================================================================
    def create_valuation(self):
        """Create Valuation Analysis sheet."""
        ws = self.wb.add_worksheet("Valuation")

        ws.merge_range('A1:J1', "Valuation Analysis", self.fmt_title)
        ws.set_row(0, 26)

        # Value Opportunities
        row = 3
        ws.merge_range(row, 0, row, 9, "Value Opportunities (Low P/E + Positive EPS)", self.fmt_section_green)
        ws.set_row(row, 22)
        row += 1

        value_stocks = []
        for s in self.stocks:
            pe = safe_float(s.get('pe_ratio'))
            eps = safe_float(s.get('eps'))
            if pe and 0 < pe < 15 and eps and eps > 0:
                value_stocks.append({
                    'ticker': s['ticker'],
                    'name': s['name'],
                    'sector': s.get('sector') or '-',
                    'price': safe_float(s['price']),
                    'pe': pe,
                    'pb': safe_float(s.get('pb_ratio')),
                    'eps': eps,
                    'bvps': safe_float(s.get('bvps')),
                    'graham': s['graham_num'],
                    'margin': s['graham_margin'],
                })

        value_stocks.sort(key=lambda x: x.get('margin') or -999, reverse=True)

        val_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 20},
            {'header': 'Sector', 'key': 'sector', 'width': 14},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': 'P/E', 'key': 'pe', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'P/B', 'key': 'pb', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'EPS', 'key': 'eps', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'BVPS', 'key': 'bvps', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Graham', 'key': 'graham', 'width': 10, 'format': self.fmt_int},
            {'header': 'Margin%', 'key': 'margin', 'width': 10, 'format': self.fmt_pct1},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, value_stocks[:30], val_cols, 'ValueStocks', TABLE_STYLE_GREEN)
        self._add_cond_fmt(ws, data_start, end_row - 1, 9, '3_color_scale',
                          min_color='#F8696B', mid_color='#FFFFFF', max_color='#63BE7B')

        # Sector Valuation
        row = end_row + 2
        ws.merge_range(row, 0, row, 3, "Sector Valuation Comparison", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        sector_pe = {}
        for s in self.stocks:
            sector = s.get('sector') or 'Unknown'
            if sector not in sector_pe:
                sector_pe[sector] = {'pe': [], 'pb': [], 'count': 0}
            sector_pe[sector]['count'] += 1
            pe = safe_float(s.get('pe_ratio'))
            pb = safe_float(s.get('pb_ratio'))
            if pe and 0 < pe < 100:
                sector_pe[sector]['pe'].append(pe)
            if pb and pb > 0:
                sector_pe[sector]['pb'].append(pb)

        def avg(lst):
            return sum(lst) / len(lst) if lst else None

        sector_data = [
            {
                'sector': k,
                'count': v['count'],
                'avg_pe': avg(v['pe']),
                'avg_pb': avg(v['pb']),
            }
            for k, v in sorted(sector_pe.items(), key=lambda x: x[1]['count'], reverse=True)
        ]

        sector_cols = [
            {'header': 'Sector', 'key': 'sector', 'width': 20},
            {'header': 'Stocks', 'key': 'count', 'width': 10, 'format': self.fmt_int},
            {'header': 'Avg P/E', 'key': 'avg_pe', 'width': 12, 'format': self.fmt_dec2},
            {'header': 'Avg P/B', 'key': 'avg_pb', 'width': 12, 'format': self.fmt_dec2},
        ]

        self._add_table(ws, row, 0, sector_data, sector_cols, 'SectorValuation')

    # =========================================================================
    # SHEET: Dividends
    # =========================================================================
    def create_dividends(self):
        """Create Dividend Analysis sheet."""
        ws = self.wb.add_worksheet("Dividends")

        ws.merge_range('A1:L1', "Dividend Analysis", self.fmt_title)
        ws.set_row(0, 26)

        # Top Dividend Yield
        row = 3
        ws.merge_range(row, 0, row, 8, "Top Dividend Yield Stocks", self.fmt_section_green)
        ws.set_row(row, 22)
        row += 1

        div_stocks = []
        for s in self.stocks:
            div = safe_float(s.get('dividend_yield'))
            if div and div > 0:
                payout = safe_float(s.get('payout_ratio'))
                de = safe_float(s.get('debt_to_equity'))
                safety = calc_dividend_safety(payout, de)
                div_stocks.append({
                    'ticker': s['ticker'],
                    'name': s['name'],
                    'sector': s.get('sector') or '-',
                    'price': safe_float(s['price']),
                    'div': div,
                    'payout': payout,
                    'pe': safe_float(s.get('pe_ratio')),
                    'de': de,
                    'safety': safety,
                })

        div_stocks.sort(key=lambda x: x['div'], reverse=True)

        div_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 20},
            {'header': 'Sector', 'key': 'sector', 'width': 14},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': 'Yield%', 'key': 'div', 'width': 10, 'format': self.fmt_pct},
            {'header': 'Payout%', 'key': 'payout', 'width': 10, 'format': self.fmt_pct},
            {'header': 'P/E', 'key': 'pe', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'D/E', 'key': 'de', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'Safety', 'key': 'safety', 'width': 10},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, div_stocks, div_cols, 'DividendStocks', TABLE_STYLE_GREEN)
        self._add_cond_fmt(ws, data_start, end_row - 1, 4, 'data_bar', bar_color=COLORS['success'])

        # Apply safety cell formatting
        for i, stock in enumerate(div_stocks):
            cell_row = row + 1 + i
            safety = stock['safety']
            if safety == 'SAFE':
                fmt = self.fmt_good
            elif safety == 'OK':
                fmt = self.fmt_warn
            else:
                fmt = self.fmt_bad
            ws.write(cell_row, 8, safety, fmt)

    # =========================================================================
    # SHEET: Financial Health
    # =========================================================================
    def create_financial_health(self):
        """Create Financial Health Analysis sheet."""
        ws = self.wb.add_worksheet("Financial Health")

        ws.merge_range('A1:L1', "Financial Health Analysis", self.fmt_title)
        ws.set_row(0, 26)

        # Most Profitable (High ROE)
        row = 3
        ws.merge_range(row, 0, row, 11, "Most Profitable Companies (by ROE)", self.fmt_section_green)
        ws.set_row(row, 22)
        row += 1

        profitable = [s for s in self.stocks if safe_float(s.get('roe'), 0) > 15]
        profitable.sort(key=lambda x: safe_float(x.get('roe'), 0), reverse=True)

        health_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 18},
            {'header': 'Sector', 'key': 'sector', 'width': 12},
            {'header': 'ROE%', 'key': 'roe', 'width': 9, 'format': self.fmt_pct},
            {'header': 'ROA%', 'key': 'roa', 'width': 9, 'format': self.fmt_pct},
            {'header': 'ROIC%', 'key': 'roic', 'width': 9, 'format': self.fmt_pct},
            {'header': 'Gross%', 'key': 'gross_margin', 'width': 9, 'format': self.fmt_pct},
            {'header': 'Op%', 'key': 'operating_margin', 'width': 8, 'format': self.fmt_pct},
            {'header': 'Net%', 'key': 'net_margin', 'width': 8, 'format': self.fmt_pct},
            {'header': 'D/E', 'key': 'debt_to_equity', 'width': 7, 'format': self.fmt_dec2},
            {'header': 'Current', 'key': 'current_ratio', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'Score', 'key': 'health_score', 'width': 7, 'format': self.fmt_score},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, profitable[:35], health_cols, 'ProfitableStocks', TABLE_STYLE_GREEN)
        self._add_cond_fmt(ws, data_start, end_row - 1, 3, 'data_bar', bar_color=COLORS['success'])
        self._add_cond_fmt(ws, data_start, end_row - 1, 9, '3_color_scale',
                          min_color='#63BE7B', mid_color='#FFEB84', max_color='#F8696B')
        self._add_cond_fmt(ws, data_start, end_row - 1, 11, 'icon_set', icon_style='3_traffic_lights')

        # All stocks financial metrics
        row = end_row + 2
        ws.merge_range(row, 0, row, 11, "All Stocks Financial Metrics", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        self._add_table(ws, row, 0, self.stocks, health_cols, 'AllFinancialMetrics')

    # =========================================================================
    # SHEET: Momentum
    # =========================================================================
    def create_momentum(self):
        """Create Momentum & Technical Analysis sheet."""
        ws = self.wb.add_worksheet("Momentum")

        ws.merge_range('A1:J1', "Momentum & Technical Analysis", self.fmt_title)
        ws.set_row(0, 26)

        # Price Momentum Matrix
        row = 3
        ws.merge_range(row, 0, row, 9, "Price Momentum Matrix", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        mom_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 18},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': '52W Pos%', 'key': 'pos_52w', 'width': 10, 'format': self.fmt_pct1},
            {'header': '1W%', 'key': 'return_1w', 'width': 8, 'format': self.fmt_pct},
            {'header': '1M%', 'key': 'return_1m', 'width': 8, 'format': self.fmt_pct},
            {'header': '3M%', 'key': 'return_3m', 'width': 8, 'format': self.fmt_pct},
            {'header': '6M%', 'key': 'return_6m', 'width': 8, 'format': self.fmt_pct},
            {'header': 'YTD%', 'key': 'return_ytd', 'width': 8, 'format': self.fmt_pct},
            {'header': '1Y%', 'key': 'return_1y', 'width': 8, 'format': self.fmt_pct},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, self.stocks, mom_cols, 'MomentumMatrix')

        n = len(self.stocks)
        self._add_cond_fmt(ws, data_start, end_row - 1, 3, 'data_bar', bar_color=COLORS['primary_light'])
        for col in [4, 5, 6, 7, 8, 9]:
            self._add_cond_fmt(ws, data_start, end_row - 1, col, 'pos_neg')

        # Near 52W High
        row = end_row + 2
        ws.merge_range(row, 0, row, 7, "Near 52-Week High (>80%)", self.fmt_section_green)
        ws.set_row(row, 22)
        row += 1

        near_high = [s for s in self.stocks if s['pos_52w'] and s['pos_52w'] > 80]
        near_high.sort(key=lambda x: x['pos_52w'], reverse=True)

        high_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 18},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': '52H', 'key': 'price_52w_high', 'width': 10, 'format': self.fmt_int},
            {'header': '52W%', 'key': 'pos_52w', 'width': 9, 'format': self.fmt_pct1},
            {'header': '1M%', 'key': 'return_1m', 'width': 8, 'format': self.fmt_pct},
            {'header': 'YTD%', 'key': 'return_ytd', 'width': 8, 'format': self.fmt_pct},
            {'header': '1Y%', 'key': 'return_1y', 'width': 8, 'format': self.fmt_pct},
        ]

        end_row = self._add_table(ws, row, 0, near_high[:25], high_cols, 'Near52High', TABLE_STYLE_GREEN)

        # Near 52W Low
        row = end_row + 2
        ws.merge_range(row, 0, row, 7, "Near 52-Week Low (<20%)", self.fmt_section_red)
        ws.set_row(row, 22)
        row += 1

        near_low = [s for s in self.stocks if s['pos_52w'] and s['pos_52w'] < 20]
        near_low.sort(key=lambda x: x['pos_52w'])

        self._add_table(ws, row, 0, near_low[:25], high_cols, 'Near52Low', TABLE_STYLE_RED)

        ws.freeze_panes(4, 2)

    # =========================================================================
    # SHEET: Sectors
    # =========================================================================
    def create_sectors(self):
        """Create Sector Comparison sheet."""
        ws = self.wb.add_worksheet("Sectors")

        ws.merge_range('A1:J1', "Sector Comparison", self.fmt_title)
        ws.set_row(0, 26)

        # Aggregate sector data
        sector_agg = {}
        for s in self.stocks:
            sector = s.get('sector') or 'Unknown'
            if sector not in sector_agg:
                sector_agg[sector] = {
                    'count': 0, 'mcap': 0,
                    'pe': [], 'pb': [], 'roe': [], 'div': [], 'ytd': [], 'de': []
                }
            sector_agg[sector]['count'] += 1
            sector_agg[sector]['mcap'] += safe_float(s.get('market_cap'), 0)

            pe = safe_float(s.get('pe_ratio'))
            if pe and 0 < pe < 100:
                sector_agg[sector]['pe'].append(pe)
            pb = safe_float(s.get('pb_ratio'))
            if pb and pb > 0:
                sector_agg[sector]['pb'].append(pb)
            roe = safe_float(s.get('roe'))
            if roe:
                sector_agg[sector]['roe'].append(roe)
            div = safe_float(s.get('dividend_yield'))
            if div and div > 0:
                sector_agg[sector]['div'].append(div)
            ytd = safe_float(s.get('return_ytd'))
            if ytd is not None:
                sector_agg[sector]['ytd'].append(ytd)
            de = safe_float(s.get('debt_to_equity'))
            if de is not None:
                sector_agg[sector]['de'].append(de)

        def avg(lst):
            return sum(lst) / len(lst) if lst else None

        sector_data = [
            {
                'sector': k,
                'count': v['count'],
                'mcap': v['mcap'] / 1e12,
                'avg_pe': avg(v['pe']),
                'avg_pb': avg(v['pb']),
                'avg_roe': avg(v['roe']),
                'avg_div': avg(v['div']),
                'avg_ytd': avg(v['ytd']),
                'avg_de': avg(v['de']),
            }
            for k, v in sorted(sector_agg.items(), key=lambda x: x[1]['mcap'], reverse=True)
        ]

        row = 3
        ws.merge_range(row, 0, row, 8, "Sector Overview", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        sector_cols = [
            {'header': 'Sector', 'key': 'sector', 'width': 20},
            {'header': 'Stocks', 'key': 'count', 'width': 8, 'format': self.fmt_int, 'total_function': 'sum'},
            {'header': 'MCap(T)', 'key': 'mcap', 'width': 10, 'format': self.fmt_dec2, 'total_function': 'sum'},
            {'header': 'Avg P/E', 'key': 'avg_pe', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Avg P/B', 'key': 'avg_pb', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Avg ROE%', 'key': 'avg_roe', 'width': 10, 'format': self.fmt_pct},
            {'header': 'Avg Yield%', 'key': 'avg_div', 'width': 10, 'format': self.fmt_pct},
            {'header': 'Avg YTD%', 'key': 'avg_ytd', 'width': 10, 'format': self.fmt_pct},
            {'header': 'Avg D/E', 'key': 'avg_de', 'width': 10, 'format': self.fmt_dec2},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, sector_data, sector_cols, 'SectorOverview', total_row=True)
        self._add_cond_fmt(ws, data_start, end_row - 2, 5, 'data_bar', bar_color=COLORS['success'])
        self._add_cond_fmt(ws, data_start, end_row - 2, 7, 'pos_neg')

    # =========================================================================
    # SHEET: News
    # =========================================================================
    def create_news(self):
        """Create News & Market Intelligence sheet."""
        ws = self.wb.add_worksheet("News")

        ws.merge_range('A1:I1', "News & Market Intelligence", self.fmt_title)
        ws.set_row(0, 26)

        query = """
        SELECT n.*, s.ticker
        FROM news n
        JOIN stocks s ON n.stock_id = s.id
        WHERE n.first_seen >= date(?, '-7 days')
        ORDER BY n.published_at DESC
        LIMIT 500
        """
        news = self.conn.execute(query, (self.date,)).fetchall()

        row = 3
        ws.merge_range(row, 0, row, 8, "Recent News (Last 7 Days)", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        news_data = []
        for n in news:
            news_data.append({
                'date': n['published_at'][:10] if n['published_at'] else '',
                'ticker': n['ticker'],
                'title': (n['title'] or '')[:70],
                'source': n['source_name'] or '',
                'category': n['category'] or '',
                'sentiment': n['sentiment'] or '',
                'score': safe_float(n['sentiment_score']),
                'critical': 'YES' if n['is_critical'] else '',
                'url': n['url'] or '',
            })

        news_cols = [
            {'header': 'Date', 'key': 'date', 'width': 11},
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Title', 'key': 'title', 'width': 60},
            {'header': 'Source', 'key': 'source', 'width': 16},
            {'header': 'Category', 'key': 'category', 'width': 12},
            {'header': 'Sentiment', 'key': 'sentiment', 'width': 10},
            {'header': 'Score', 'key': 'score', 'width': 8, 'format': self.fmt_dec2},
            {'header': 'Critical', 'key': 'critical', 'width': 8},
            {'header': 'URL', 'key': 'url', 'width': 50},
        ]

        end_row = self._add_table(ws, row, 0, news_data, news_cols, 'NewsData')

        # Apply sentiment and critical highlighting
        for i, n in enumerate(news_data):
            cell_row = row + 1 + i
            if n['critical'] == 'YES':
                ws.write(cell_row, 7, 'YES', self.fmt_bad)
            if n['sentiment'] == 'positive':
                ws.write(cell_row, 5, n['sentiment'], self.fmt_good)
            elif n['sentiment'] == 'negative':
                ws.write(cell_row, 5, n['sentiment'], self.fmt_bad)

        ws.freeze_panes(4, 2)

    # =========================================================================
    # SHEET: Sentiment
    # =========================================================================
    def create_sentiment(self):
        """Create Market Sentiment Analysis sheet."""
        ws = self.wb.add_worksheet("Sentiment")

        ws.merge_range('A1:F1', "Market Sentiment Analysis", self.fmt_title)
        ws.set_row(0, 26)

        sentiment_stocks = [s for s in self.stocks if s.get('bullish_pct') is not None]

        # Most Bullish
        row = 3
        ws.merge_range(row, 0, row, 5, "Most Bullish Stocks", self.fmt_section_green)
        ws.set_row(row, 22)
        row += 1

        sent_data = []
        for s in sentiment_stocks:
            sent_data.append({
                'ticker': s['ticker'],
                'name': s['name'],
                'price': safe_float(s['price']),
                'bull': safe_float(s.get('bullish_pct')),
                'bear': safe_float(s.get('bearish_pct')),
                'net': (safe_float(s.get('bullish_pct'), 0) - safe_float(s.get('bearish_pct'), 0)),
            })

        sent_data.sort(key=lambda x: x['net'], reverse=True)

        sent_cols = [
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 20},
            {'header': 'Price', 'key': 'price', 'width': 10, 'format': self.fmt_int},
            {'header': 'Bull%', 'key': 'bull', 'width': 10, 'format': self.fmt_pct1},
            {'header': 'Bear%', 'key': 'bear', 'width': 10, 'format': self.fmt_pct1},
            {'header': 'Net', 'key': 'net', 'width': 10, 'format': self.fmt_pct1},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, sent_data, sent_cols, 'SentimentData')
        self._add_cond_fmt(ws, data_start, end_row - 1, 3, 'data_bar', bar_color=COLORS['success'])
        self._add_cond_fmt(ws, data_start, end_row - 1, 4, 'data_bar', bar_color=COLORS['danger'])
        self._add_cond_fmt(ws, data_start, end_row - 1, 5, '3_color_scale')

        # Most Bearish
        row = end_row + 2
        ws.merge_range(row, 0, row, 5, "Most Bearish Stocks", self.fmt_section_red)
        ws.set_row(row, 22)
        row += 1

        bearish_data = sorted(sent_data, key=lambda x: x['net'])

        self._add_table(ws, row, 0, bearish_data[:20], sent_cols, 'BearishStocks', TABLE_STYLE_RED)

    # =========================================================================
    # SHEET: Earnings
    # =========================================================================
    def create_earnings(self):
        """Create Earnings History & Performance sheet."""
        ws = self.wb.add_worksheet("Earnings")

        ws.merge_range('A1:K1', "Earnings History & Performance", self.fmt_title)
        ws.set_row(0, 26)

        query = """
        SELECT e.*, s.ticker, s.name
        FROM earnings e
        JOIN stocks s ON e.stock_id = s.id
        ORDER BY e.event_date DESC
        LIMIT 300
        """
        earnings = self.conn.execute(query).fetchall()

        row = 3
        ws.merge_range(row, 0, row, 9, "Recent Earnings Reports", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        earn_data = []
        for e in earnings:
            surprise = safe_float(e['eps_surprise_pct'], 0)
            result = 'BEAT' if surprise > 0 else ('MISS' if surprise < 0 else '-')
            earn_data.append({
                'date': e['event_date'] or '',
                'ticker': e['ticker'],
                'name': e['name'],
                'fy': e['fiscal_year'],
                'fq': e['fiscal_quarter'],
                'eps_est': safe_float(e['eps_estimate']),
                'eps_act': safe_float(e['eps_actual']),
                'surprise': safe_float(e['eps_surprise']),
                'surprise_pct': surprise,
                'result': result,
            })

        earn_cols = [
            {'header': 'Date', 'key': 'date', 'width': 11},
            {'header': 'Ticker', 'key': 'ticker', 'width': 8},
            {'header': 'Name', 'key': 'name', 'width': 20},
            {'header': 'FY', 'key': 'fy', 'width': 6, 'format': self.fmt_int},
            {'header': 'FQ', 'key': 'fq', 'width': 5, 'format': self.fmt_int},
            {'header': 'EPS Est', 'key': 'eps_est', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'EPS Act', 'key': 'eps_act', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Surprise', 'key': 'surprise', 'width': 10, 'format': self.fmt_dec2},
            {'header': 'Surp%', 'key': 'surprise_pct', 'width': 9, 'format': self.fmt_pct},
            {'header': 'Result', 'key': 'result', 'width': 8},
        ]

        data_start = row + 1
        end_row = self._add_table(ws, row, 0, earn_data, earn_cols, 'EarningsData')
        self._add_cond_fmt(ws, data_start, end_row - 1, 8, '3_color_scale')

        # Apply result highlighting
        for i, e in enumerate(earn_data):
            cell_row = row + 1 + i
            result = e['result']
            if result == 'BEAT':
                fmt = self.fmt_good
            elif result == 'MISS':
                fmt = self.fmt_bad
            else:
                fmt = self.fmt_neutral
            ws.write(cell_row, 9, result, fmt)

        # Earnings Summary
        row = end_row + 2
        ws.merge_range(row, 0, row, 5, "Earnings Summary", self.fmt_section)
        ws.set_row(row, 22)
        row += 1

        beats = sum(1 for e in earn_data if e['result'] == 'BEAT')
        misses = sum(1 for e in earn_data if e['result'] == 'MISS')
        total = len(earn_data)

        summary_data = [
            {'metric': 'Total Reports', 'value': total},
            {'metric': 'Beats', 'value': beats},
            {'metric': 'Misses', 'value': misses},
            {'metric': 'Beat Rate %', 'value': (beats / total * 100) if total else 0},
        ]

        summary_cols = [
            {'header': 'Metric', 'key': 'metric', 'width': 16},
            {'header': 'Value', 'key': 'value', 'width': 12, 'format': self.fmt_dec2},
        ]

        self._add_table(ws, row, 0, summary_data, summary_cols, 'EarningsSummary')

        ws.freeze_panes(4, 2)

    # =========================================================================
    # Generate All Sheets
    # =========================================================================
    def generate(self):
        """Generate all dashboard sheets."""
        print("Creating Executive Summary...")
        self.create_summary()

        print("Creating All Stocks...")
        self.create_all_stocks()

        print("Creating Valuation...")
        self.create_valuation()

        print("Creating Dividends...")
        self.create_dividends()

        print("Creating Financial Health...")
        self.create_financial_health()

        print("Creating Momentum...")
        self.create_momentum()

        print("Creating Sectors...")
        self.create_sectors()

        print("Creating News...")
        self.create_news()

        print("Creating Sentiment...")
        self.create_sentiment()

        print("Creating Earnings...")
        self.create_earnings()


# ============================================================================
# Main Entry Point
# ============================================================================

def get_latest_date(conn: sqlite3.Connection) -> str:
    """Get latest scrape date from database."""
    cursor = conn.execute("SELECT MAX(scrape_date) FROM price_history")
    result = cursor.fetchone()
    return result[0] if result[0] else datetime.now().strftime("%Y-%m-%d")


def main():
    parser = argparse.ArgumentParser(
        description='Create professional Excel dashboard from stock data')
    parser.add_argument('--db', required=True, help='SQLite database path')
    parser.add_argument('--output', '-o', help='Output Excel file path')
    parser.add_argument('--date', help='Scrape date (default: latest)')
    args = parser.parse_args()

    if not Path(args.db).exists():
        print(f"Error: Database not found: {args.db}")
        return 1

    conn = sqlite3.connect(args.db)
    conn.row_factory = sqlite3.Row

    date = args.date or get_latest_date(conn)
    print(f"Using data from: {date}")

    output_path = args.output or f"output/dashboard_{date.replace('-', '')}.xlsx"

    workbook = xlsxwriter.Workbook(output_path, {
        'constant_memory': False,
        'strings_to_urls': True,
    })

    dashboard = ProfessionalDashboard(workbook, conn, date)
    dashboard.generate()

    workbook.close()
    conn.close()

    print(f"\nDashboard saved to: {output_path}")
    return 0


if __name__ == '__main__':
    exit(main())
