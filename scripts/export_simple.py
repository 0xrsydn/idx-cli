#!/usr/bin/env python3
"""Simple table export from SQLite to JSON/CSV/XLSX."""

import argparse
import json
import sqlite3
from pathlib import Path

import pandas as pd

DEFAULT_TABLES = [
    "stocks",
    "price_history",
    "ratios_history",
    "news",
    "sentiment_history",
    "scrape_runs",
    "scrape_progress",
]


def load_table(conn: sqlite3.Connection, table: str) -> pd.DataFrame:
    return pd.read_sql_query(f"SELECT * FROM {table}", conn)


def export_json(conn: sqlite3.Connection, outdir: Path, tables: list[str]) -> int:
    outdir.mkdir(parents=True, exist_ok=True)
    for table in tables:
        df = load_table(conn, table)
        data = json.loads(df.to_json(orient="records", date_format="iso"))
        (outdir / f"{table}.json").write_text(json.dumps(data, indent=2), encoding="utf-8")
    return 0


def export_csv(conn: sqlite3.Connection, outdir: Path, tables: list[str]) -> int:
    outdir.mkdir(parents=True, exist_ok=True)
    for table in tables:
        df = load_table(conn, table)
        df.to_csv(outdir / f"{table}.csv", index=False)
    return 0


def export_xlsx(conn: sqlite3.Connection, outfile: Path, tables: list[str]) -> int:
    outfile.parent.mkdir(parents=True, exist_ok=True)
    with pd.ExcelWriter(outfile, engine="openpyxl") as writer:
        for table in tables:
            df = load_table(conn, table)
            sheet = table[:31] if table else "sheet"
            df.to_excel(writer, sheet_name=sheet, index=False)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Simple SQLite table exporter")
    parser.add_argument("--db", required=True, help="SQLite database path")
    parser.add_argument("--format", required=True, choices=["json", "csv", "xlsx"], help="Export format")
    parser.add_argument("--output", "-o", required=True, help="Output path (dir for json/csv, file for xlsx)")
    parser.add_argument("--tables", help="Comma-separated tables (default: common tables)")
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Error: Database not found: {db_path}")
        return 1

    tables = [t.strip() for t in args.tables.split(",")] if args.tables else DEFAULT_TABLES
    tables = [t for t in tables if t]

    conn = sqlite3.connect(db_path)
    try:
        if args.format == "json":
            return export_json(conn, Path(args.output), tables)
        if args.format == "csv":
            return export_csv(conn, Path(args.output), tables)
        return export_xlsx(conn, Path(args.output), tables)
    finally:
        conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
