# Refuse non-sequential JPEG before entropy decode

The 853-byte progressive timeout fixture SHA256 is b3157d2e81d99081bbbaed4a7705fbae9f8e98c136101b88f665a639d3821810. Current zune-jpeg progressive decoding lacks an enforced work limit; pixel/byte caps and a 64-scan cap did not prevent a fuzz timeout. Shared JPEG preflight now accepts only sequential Huffman SOF0/SOF1. All other SOF modes, including progressive SOF2, are refused by size, grey decode and colour decode before entering the image decoder.

This deliberately removes progressive JPEG support until decoding has a proven work budget. Baseline, greyscale, CMYK and EXIF JPEG tests still pass. Existing truncated final-entropy recovery still passes. Browser tests explicitly require progressive-image fallback instead of pretending it decodes. Not a general CPU deadline for baseline JPEG or PNG.

Own validation:
- Image full suite: 25 unit tests plus 4 JPEG budget tests and 2 new work-limit tests, all pass.
- Host release exact853B fixture probe: grey refusal 2 microseconds, colour refusal below 1 microsecond.
- Host release 3000 shared-entry refusals: 71.896 microseconds total; valid progressive and fuzz fixture both refused.
- Browser full updated-binary inventory: 296 tests, 290 passed, 6 ignored, zero failures.
- Actual dirty-head realTLS app: external PNG and two sheets paint; SVG not requested; address Cancel/Back restore frame. app.png pixels inspected. 66160 KiB peak RSS in this single run, not a comparative measurement.
- ARM/qemu release probe and size/RSS qualification pending parent.

Includes earlier slot-starvation and hidden-ancestor fixes. Prior webdoc/net tests and corpus results belong to d781; no changed webdoc/net implementation here. Fresh corpus check pending at this commit.
