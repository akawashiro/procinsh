"""Exercise live gzip/identity streams including fragmented SSE delimiters."""
import http.server
import json
import threading
import time
import unittest
import zlib

from measure_http_compression import measure


class MeasurementTests(unittest.TestCase):
    def test_live_streams(self):
        for encoding in ('identity', 'gzip'):
            with self.subTest(encoding=encoding):
                class Handler(http.server.BaseHTTPRequestHandler):
                    def log_message(self, *args):
                        pass

                    def do_GET(self):
                        self.send_response(200)
                        if encoding == 'gzip':
                            self.send_header('Content-Encoding', 'gzip')
                        self.end_headers()
                        compressor = zlib.compressobj(wbits=31)
                        status = {name: {'state': 'observing'} for name in ('ipc', 'cpu', 'files')}
                        payloads = [('snapshot', {'captured_at': int(time.time() * 1000)}),
                                    ('activity', {'captured_at': int(time.time() * 1000), 'status': status}),
                                    ('gap', {'dropped_frames': 7})]
                        for name, value in payloads:
                            event = f'event: {name}\ndata: {json.dumps(value)}\n\n'.encode()
                            wire = compressor.compress(event) + compressor.flush(zlib.Z_SYNC_FLUSH) if encoding == 'gzip' else event
                            # Splitting the final SSE delimiter across reads is valid.
                            self.wfile.write(wire[:-1])
                            self.wfile.flush()
                            time.sleep(0.005)
                            self.wfile.write(wire[-1:])
                            self.wfile.flush()
                            time.sleep(0.005)
                        time.sleep(0.3)  # Keep the stream open past the measurement window.

                with http.server.HTTPServer(('127.0.0.1', 0), Handler) as server:
                    thread = threading.Thread(target=server.serve_forever)
                    thread.start()
                    try:
                        result = measure(f'http://127.0.0.1:{server.server_port}', 0.15, encoding)
                    finally:
                        server.shutdown()
                        thread.join()
                self.assertEqual(result['dropped_frames'], 7)
                self.assertGreater(result['transferred_bytes'], 0)
                for name in ('snapshot', 'activity', 'gap'):
                    self.assertEqual(result['events'][name]['event_count'], 1)
                if encoding == 'identity':
                    self.assertEqual(result['raw_sse_bytes'], result['transferred_bytes'])


if __name__ == '__main__':
    unittest.main()
