# Page view fallback subsets

Sources: Debian fonts-noto-core 20201225-1build1, NotoSans-Regular.ttf and
NotoSansDevanagari-Regular.ttf. Licence: SIL OFL 1.1, LICENSE-Noto.txt.

Original SHA-256 values:

- NotoSans-Regular.ttf: 89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d
- NotoSansDevanagari-Regular.ttf: 79a470365ccb210fa3c7d8d8ff2e005ef9d983cfd067f735a0caf7e15070ca9f

The modified fonts are renamed Kobo Page Cyrillic and Kobo Page Devanagari.
All layout features are retained. These fonts supply regular fallback glyphs;
matching bold and italic fallback cuts are not bundled.

Rebuild using fontTools subset with layout_features=['*'], name_IDs=['*']:

- Cyrillic: U+0020-007E, U+0400-052F.
- Devanagari: U+0020-007E, U+0900-097F, U+A8E0-A8FF, U+2000-206F.

After subsetting, replace name IDs 1, 4, 6 and 16 with the new family name.
Name ID 6 has spaces removed and a -Regular suffix. Save as TrueType.
Checksums and sizes can differ across fontTools versions; the checked-in bytes
are the build inputs. Do not regenerate from an arbitrary newer font version.

CJK is intentionally not bundled: the broad Japanese subset costs 13.38 MB.
Callers may install an external face through PageFontSession::add_fallback.

For the bundled Hindi and Russian fixtures, no external face is needed.
The Japanese fixture intentionally exercises the optional fallback path.
A zero skipped-block count alone is not proof of glyph coverage or shaping.

Additional style cuts use the same ranges and naming rules: NotoSans-Bold,
NotoSans-Italic, NotoSans-BoldItalic and NotoSansDevanagari-Bold, from the
same Debian distribution. These are selected by actual weight and slant.
No Devanagari italic is synthesized. Catalog capacity is 16 explicit faces.

Arabic and Hebrew regular are derived from NotoSansArabic-Regular.ttf and
NotoSansHebrew-Regular.ttf in the same Debian distribution. Arabic ranges:
U+0020-007E, U+0600-06FF, U+0750-077F, U+08A0-08FF, U+FB50-FDFF,
U+FE70-FEFF. Hebrew ranges: U+0020-007E, U+0590-05FF, U+FB1D-FB4F.
Modified names are Kobo Page Arabic and Kobo Page Hebrew. These sources have
no italic cuts; none is synthesized.

Arabic bold and Hebrew bold use the same ranges as their regular cuts, from
NotoSansArabic-Bold.ttf and NotoSansHebrew-Bold.ttf, same source distribution.
Renamed families stay Kobo Page Arabic and Kobo Page Hebrew, style Bold.
