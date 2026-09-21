"""Small HTTP SSE client with deterministic shutdown of a blocked reader."""
import http.client
import socket
from urllib.parse import urlsplit


class Stream:
    def __init__(self, url, timeout=20):
        parsed = urlsplit(url)
        connection = (http.client.HTTPSConnection if parsed.scheme == 'https'
                      else http.client.HTTPConnection)
        self.connection = connection(parsed.hostname, parsed.port, timeout=timeout)
        try:
            self.connection.request('GET', parsed.path or '/')
            self.socket = self.connection.sock
            self.response = self.connection.getresponse()
            if self.response.status != 200:
                raise RuntimeError(f'SSE HTTP {self.response.status}')
        except BaseException:
            self.connection.close()
            raise

    def __iter__(self):
        return iter(self.response)

    def close(self, reader=None):
        try:
            self.socket.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        if reader:
            reader.join(timeout=5)
        self.response.close()
        self.connection.close()
        if reader and reader.is_alive():
            raise RuntimeError('SSE reader did not stop')
