"""Fetch only the pinned CC0 recordings used by Ahess; verify Git blob hashes."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import sys
import urllib.parse
import subprocess

REVISION = 'b3b3249d37dc977a1a297bd2dc053e6d9b6b805c'
root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
sources = json.loads(Path(__file__).with_name('sources.json').read_text())

def download(source):
    path = root / source['path']
    if not path.exists():
        url = f'https://raw.githubusercontent.com/sfzinstruments/karoryfer.black-and-green-guitars/{REVISION}/' + urllib.parse.quote(source['path'])
        data = subprocess.check_output(['curl', '--fail', '--location', '--silent', '--show-error', '--retry', '3', '--max-time', '90', url])
        assert len(data) == source['size']
        assert hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest() == source['sha']
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    data = path.read_bytes()
    assert hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest() == source['sha']
    return source['path']

with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
    for name in pool.map(download, sources):
        print(name, flush=True)
