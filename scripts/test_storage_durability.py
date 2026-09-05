"""Check both startup schemas retain durable commits before sync acknowledgments."""
import pathlib
import re
import sqlite3
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SQLITE_SYNCHRONOUS_FULL = 2


class StorageDurabilityTests(unittest.TestCase):
    def test_acknowledged_commits_remain_durable(self):
        for path in ('server/src/storage.rs', 'src-tauri/src/storage.rs'):
            with self.subTest(path=path), sqlite3.connect(':memory:') as connection:
                source = (ROOT / path).read_text()
                schema = re.search(r'conn.execute_batch\(\s*"(.*?)",\s*\)\?', source, re.S)
                self.assertIsNotNone(schema)
                connection.executescript(schema.group(1))
                synchronous = connection.execute('PRAGMA synchronous').fetchone()[0]
                self.assertEqual(synchronous, SQLITE_SYNCHRONOUS_FULL)


if __name__ == '__main__':
    unittest.main()
