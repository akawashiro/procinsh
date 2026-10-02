import gzip
import unittest
from measure_system_traffic import fields, percentile, size, snapshot


class MeasurementTests(unittest.TestCase):
    def test_exact_utf8_and_escaped_values(self):
        raw = r'{"processes":[{"name":"日本語","maps":["\u0061","x\\y"]}],"fd_relations":[],"warnings":[]}'
        report = snapshot(raw)
        self.assertEqual(report['maps_value_bytes'], size(r'["\u0061","x\\y"]'))
        self.assertEqual(report['map_entry_count'], 2)
        self.assertEqual(report['process_count'], 1)
        self.assertEqual(report['raw_bytes'], sum(report[k] for k in ('processes_bytes', 'fd_relations_bytes', 'other_bytes')))
        self.assertEqual(report['gzip_bytes'], len(gzip.compress(raw.encode(), compresslevel=6, mtime=0)))
        self.assertEqual(fields('{ "a" : {}, "b": [] }'), {'a': '{}', 'b': '[]'})

    def test_empty_snapshot_and_percentiles(self):
        self.assertEqual(snapshot('{"processes":[],"fd_relations":[]}')['maps_value_bytes'], 0)
        self.assertIsNone(percentile([], 99))
        self.assertEqual(percentile(list(range(1, 101)), 95), 95)


if __name__ == '__main__':
    unittest.main()
