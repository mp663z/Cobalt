# Host text preview

This restricted TrueType preview is a standalone host tool, not a device
workspace package. Its fonts and lockfile do not change Store app dependencies.
The background-only frozen benchmarks remain unchanged.

From the repository root:

```
cargo run --locked --manifest-path tools/cobalt-text-preview/Cargo.toml -- \
  page.html preview.ppm 800 600 /path/to/font.ttf --blocks
```

Omit `--blocks` for the single-text-block mode. Unsupported content is refused,
not silently painted as an empty page.
