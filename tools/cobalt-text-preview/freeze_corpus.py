"""One-time corpus acquisition. Run only to intentionally refresh sources.

Pages and linked stylesheets are stored unmodified. manifest.json retains
source URLs and SHA-256 values. No cookies or scripts are evaluated.
"""
import hashlib
import json
from datetime import datetime, timezone
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urljoin
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parent / 'corpus'
PAGES = {
    'python-introduction': 'https://docs.python.org/3/tutorial/introduction.html',
    'python-controlflow': 'https://docs.python.org/3/tutorial/controlflow.html',
    'python-datastructures': 'https://docs.python.org/3/tutorial/datastructures.html',
    'python-modules': 'https://docs.python.org/3/tutorial/modules.html',
    'python-errors': 'https://docs.python.org/3/tutorial/errors.html',
    'rust-installation': 'https://doc.rust-lang.org/book/ch01-01-installation.html',
    'rust-hello': 'https://doc.rust-lang.org/book/ch01-02-hello-world.html',
    'rust-guessing': 'https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html',
    'rust-variables': 'https://doc.rust-lang.org/book/ch03-01-variables-and-mutability.html',
    'rust-functions': 'https://doc.rust-lang.org/book/ch03-03-how-functions-work.html',
}
class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'link' and 'stylesheet' in attrs.get('rel', '').split() and 'href' in attrs:
            self.links.append(attrs['href'])

def download(url, path):
    with urlopen(url, timeout=60) as response:
        data = response.read()
        resolved = response.url
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return {'url': url, 'resolved_url': resolved, 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}

def main():
    synthetic = [page for page in json.loads((ROOT / 'manifest.json').read_text())['pages'] if 'source_path' in page]
    manifest = {'fetched_at': datetime.now(timezone.utc).isoformat(), 'pages': []}
    for name, url in PAGES.items():
        directory = ROOT / name
        page = download(url, directory / 'page.html')
        parser = Links()
        parser.feed((directory / 'page.html').read_text())
        page.update(name=name, license='PSF License 2' if name.startswith('python-') else 'MIT OR Apache-2.0')
        page['stylesheets'] = [download(urljoin(url, link), directory / f'sheet-{i}.css') for i, link in enumerate(parser.links)]
        manifest['pages'].append(page)
        print(name, page['bytes'], len(page['stylesheets']), flush=True)
    manifest['pages'].extend(synthetic)
    (ROOT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
if __name__ == '__main__':
    main()
