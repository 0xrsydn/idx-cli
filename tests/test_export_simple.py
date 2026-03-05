import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from scripts.export_simple import export_csv, export_json, export_xlsx


class ExportSimpleTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)
        self.db = self.base / "test.db"
        conn = sqlite3.connect(self.db)
        conn.execute("CREATE TABLE stocks (id TEXT, ticker TEXT)")
        conn.execute("INSERT INTO stocks VALUES ('1','BBCA')")
        conn.commit()
        conn.close()

    def tearDown(self):
        self.tmp.cleanup()

    def test_export_json(self):
        conn = sqlite3.connect(self.db)
        out = self.base / "json"
        export_json(conn, out, ["stocks"])
        conn.close()
        data = json.loads((out / "stocks.json").read_text())
        self.assertEqual(data[0]["ticker"], "BBCA")

    def test_export_csv(self):
        conn = sqlite3.connect(self.db)
        out = self.base / "csv"
        export_csv(conn, out, ["stocks"])
        conn.close()
        text = (out / "stocks.csv").read_text()
        self.assertIn("BBCA", text)

    def test_export_xlsx(self):
        conn = sqlite3.connect(self.db)
        out = self.base / "out.xlsx"
        export_xlsx(conn, out, ["stocks"])
        conn.close()
        self.assertTrue(out.exists())


if __name__ == "__main__":
    unittest.main()
