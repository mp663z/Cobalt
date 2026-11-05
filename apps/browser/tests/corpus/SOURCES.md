# Corpus sources

Real pages, fetched once and shrunk with `minimize.py`. The shrinking keeps
what the browser reads; before these were committed, each shrunk page was
checked to give the same title, warnings, visible text and link list as the
page it came from.

All were fetched on 2026-09-25 (UTC) with plain `curl -L`, HTTP 200, no
cookies.

| File | Fetched from | Licence of the content |
| --- | --- | --- |
| `wikipedia-e-reader.html` | https://en.wikipedia.org/wiki/E-reader | CC BY-SA 4.0, Wikipedia contributors |
| `wikipedia-search.html` | https://en.wikipedia.org/w/index.php?search=e-ink+display&fulltext=1&ns0=1 | CC BY-SA 4.0, Wikipedia contributors |
| `rust-book-installation.html` | https://doc.rust-lang.org/book/ch01-01-installation.html | MIT or Apache 2.0, The Rust Project |
| `rust-std-option.html` | https://doc.rust-lang.org/std/option/index.html | MIT or Apache 2.0, The Rust Project |
| `rust-blog-post.html` | https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/ | MIT or Apache 2.0, The Rust Project |
| `python-tutorial-intro.html` | https://docs.python.org/3/tutorial/introduction.html | PSF License 2, Python Software Foundation |

Two bad pages are made from these, not fetched:

- `cut-short.html`: the Wikipedia article cut off inside a link's start tag,
  a little under 60 KB in, as a dropped connection leaves it.
- `windows-1252.html`: the Python tutorial page re-encoded as Windows-1252,
  its charset declaration changed to match.

Hacker News was fetched too but is not included: its content has no licence
that allows copying it here.
