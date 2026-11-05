# Pure inline CSS admission, stage 1

`data_stylesheet::decode` is a pure strict decoder, not a resource loader. It is
not called by the parser, cascade, shipping preview, navigation or network path.
No existing flags, limits, frozen bytes or caller behavior change. All five real
pages still refuse under the present resource/input/CSS budgets.

Supported profile: ASCII-case-insensitive `data:text/css;base64,` or
`data:text/css;charset=utf-8;base64,`. Payload is case-sensitive canonical RFC4648
base64. Other media, charsets, parameters, parameter ordering, raw/URL-escaped
payloads, whitespace, fragments and noncanonical padding/tail bits are refused.
Unsupported profiles are not described as invalid Web URLs. UTF-8 BOM and empty
CSS are retained exactly; invalid UTF-8 and decoded NUL refuse. The latter avoids
pretending the CSS tokenizer's replacement-character rules are implemented.

Decoded size uses checked arithmetic and the existing scanner's 256KiB ceiling.
The encoded limit is derived from that ceiling, not a new resource budget. Exact
output allocation uses try_reserve_exact before pushes; errors contain only stable
enum variants, never URL/payload text. The immutable result exposes a borrowed CSS
string and encoded/decoded lengths. It grants no page-level admission, sheet-count
exception, aggregate-memory allowance, DOM identity or permission to clear loss.

Stage 2 remains gated on reviewed DOM-handle/source binding, original token and
attribute merge/reparenting semantics, explicit aggregate in-flight page memory
accounting, disabled/type/media rules and no-network/import behavior. No synthetic
hrefs, shortened URIs, frozen HTML rewrites or silent truncation-flag removal.

Sources:
https://www.rfc-editor.org/rfc/rfc2397
https://www.rfc-editor.org/rfc/rfc4648

Tests cover RFC vectors, ASCII roundtrips across padding boundaries, exact UTF-8/
BOM bytes, empty CSS, header case, unsupported profiles, illegal alphabet/padding/
tail bits, NUL/UTF-8, exact/encoded/decoded/header budgets and arithmetic overflow.
A caller-isolation test confirms decoding does not change a subsequent parser's
missing-stylesheet refusal. There is no IO or networking dependency in this module.
