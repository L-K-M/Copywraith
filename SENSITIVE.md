# Sensitive-data handling

Copywraith tries to recognize secrets that land on the clipboard (API keys,
tokens, passwords, card numbers, etc.) and avoid exposing them. Detection lives
in `crates/copywraith-core/src/sensitive.rs`; masking lives in
`crates/copywraith-core/src/content.rs` (`mask_sensitive`).

## What happens to a sensitive entry

When an entry is created (client or server), its best plain-text representation
is run through `contains_sensitive_data`. If it matches, the entry's `sensitive`
flag is set, and from then on:

- **Previews and full text are masked** before leaving Rust: the UI never
  receives the full secret. Masking shows the first few characters followed by
  bullet characters (`mask_sensitive`).
- The **server masks** sensitive text in API responses too
  (`mask_sensitive_entry` in `server/src/api.rs`), and drops the HTML/RTF/file
  flavors for sensitive entries so the secret can't leak through an alternate
  flavor.
- The entry is still stored and still syncs; only its *display* is masked. (It
  is encrypted at rest on the server like any other entry.)

## What is detected

`sensitive.rs` combines specific high-signal patterns with a couple of
entropy/heuristic checks. Categories include:

- **Cloud / provider keys**: AWS access keys, OpenAI-style keys, and other
  prefixed tokens.
- **JWTs**: three base64url segments separated by dots.
- **Private keys**: PEM `-----BEGIN ... PRIVATE KEY-----` blocks.
- **Credit-card numbers**: digit sequences that pass a Luhn check.
- **US SSNs**: with area/group/serial validity rules to reduce false positives.
- **Assignments**: `password=…`, `secret=…`, `token=…`, `api_key=…`, etc.
- **Standalone secrets**: a single high-entropy token, or a token mixing
  letters/digits/symbols, that looks generated rather than prose.

The unit tests at the bottom of `sensitive.rs` document both the positive cases
and the negatives that must *not* trip (normal numbers, short text, common
non-secret words).

## Design bias

Detection deliberately errs toward **false positives over false negatives**: it
is better to mask a harmless string than to display a real secret. As a result,
some high-entropy non-secrets (long IDs, hashes) may be masked.

## Known gaps / future work

- Assignment patterns stop at the first whitespace, so quoted values containing
  spaces (`password = "two words"`) are only partially matched.
- The JWT and generic-secret patterns are structural, not semantic, so they can
  match innocent base64-looking data.
- Entropy thresholds are heuristic; tuning them changes the false-positive rate.
- There is no per-entry override yet (e.g. "treat this as not sensitive").
</content>
