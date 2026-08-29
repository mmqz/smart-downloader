# Bug B 复现基建：支持 HTTP Range 的最小静态源（http.server 默认不支持 Range，
# 而 HttpEngine 多段下载需要 206）。用法: python range_server.py <port> <dir>
import http.server
import os
import re
import socketserver
import sys


class RangeHandler(http.server.SimpleHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def _send_file(self, head_only=False):
        path = self.translate_path(self.path)
        if not os.path.isfile(path):
            self.send_error(404, 'not found')
            return
        size = os.fstat(os.open(path, os.O_RDONLY | os.O_SEQUENTIAL)).st_size \
            if hasattr(os, 'O_SEQUENTIAL') else os.path.getsize(path)
        rng = self.headers.get('Range')
        m = re.match(r'bytes=(\d*)-(\d*)$', rng.strip()) if rng else None
        if rng and m and (m.group(1) or m.group(2)):
            if m.group(1) == '':
                start = max(0, size - int(m.group(2)))
                end = size - 1
            else:
                start = int(m.group(1))
                end = min(size - 1, int(m.group(2))) if m.group(2) else size - 1
            if start > end or start >= size:
                self.send_response(416)
                self.send_header('Content-Range', f'bytes */{size}')
                self.end_headers()
                return
            length = end - start + 1
            self.send_response(206)
            self.send_header('Content-Type', 'application/octet-stream')
            self.send_header('Content-Range', f'bytes {start}-{end}/{size}')
            self.send_header('Content-Length', str(length))
            self.send_header('Accept-Ranges', 'bytes')
            self.end_headers()
            if head_only:
                return
            with open(path, 'rb') as f:
                f.seek(start)
                remain = length
                while remain > 0:
                    chunk = f.read(min(65536, remain))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    remain -= len(chunk)
            return
        self.send_response(200)
        self.send_header('Content-Type', 'application/octet-stream')
        self.send_header('Content-Length', str(size))
        self.send_header('Accept-Ranges', 'bytes')
        self.end_headers()
        if head_only:
            return
        with open(path, 'rb') as f:
            while True:
                chunk = f.read(65536)
                if not chunk:
                    break
                self.wfile.write(chunk)

    def do_HEAD(self):
        self._send_file(head_only=True)

    def do_GET(self):
        self._send_file(head_only=False)

    def log_message(self, fmt, *args):
        pass


def main():
    port = int(sys.argv[1])
    directory = sys.argv[2]
    os.chdir(directory)


    class Server(socketserver.ThreadingTCPServer):
        allow_reuse_address = True
        daemon_threads = True

    httpd = Server(('127.0.0.1', port), RangeHandler)
    print(f'RANGE-SERVER READY {port}', flush=True)
    httpd.serve_forever()


if __name__ == '__main__':
    main()
