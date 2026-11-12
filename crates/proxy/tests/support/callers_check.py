# Run by callers.rs's test with sonde's hook on PYTHONPATH: a tiny S3 that
# notes each request's x-sonde-caller, and a program that sends to it with
# boto3, once directly and once through the transfer manager's threads.
# Prints one line per request: method, query, caller.
import io
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

seen = []


class S3(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def answer(self, body=b"", headers=()):
        self.send_response(200)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def note(self):
        n = int(self.headers.get("Content-Length") or 0)
        if n:
            self.rfile.read(n)
        q = self.path.split("?", 1)[1] if "?" in self.path else ""
        seen.append((self.command, q.split("=")[0], self.headers.get("x-sonde-caller") or "-"))
        return q

    def do_PUT(self):
        self.note()
        self.answer(headers=[("ETag", '"e"')])

    def do_POST(self):
        q = self.note()
        if q.startswith("uploads"):
            body = b"<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key><UploadId>u1</UploadId></InitiateMultipartUploadResult>"
        else:
            body = b"<CompleteMultipartUploadResult><Bucket>b</Bucket><Key>k</Key><ETag>\"e\"</ETag></CompleteMultipartUploadResult>"
        self.answer(body)


server = ThreadingHTTPServer(("127.0.0.1", 0), S3)
threading.Thread(target=server.serve_forever, daemon=True).start()
for k in ("HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"):
    os.environ.pop(k, None)
os.environ["AWS_ENDPOINT_URL"] = "http://127.0.0.1:%d" % server.server_port
os.environ.setdefault("AWS_ACCESS_KEY_ID", "k")
os.environ.setdefault("AWS_SECRET_ACCESS_KEY", "s")

import boto3  # noqa: E402
from boto3.s3.transfer import TransferConfig  # noqa: E402
from botocore.config import Config  # noqa: E402

s3 = boto3.client("s3", region_name="us-east-1", config=Config(s3={"addressing_style": "path"}))


def put():
    s3.put_object(Bucket="b", Key="k", Body=b"x")


def upload():
    cfg = TransferConfig(multipart_threshold=5 * 1024 * 1024, multipart_chunksize=5 * 1024 * 1024)
    s3.upload_fileobj(io.BytesIO(b"y" * (11 * 1024 * 1024)), "b", "k", Config=cfg)


put()
upload()
server.shutdown()
for m, q, c in seen:
    print(m, q or "-", c)
sys.stdout.flush()
