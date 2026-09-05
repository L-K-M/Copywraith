/**
 * Plain-text extraction for clipboard previews.
 *
 * Lives outside EntryRow.svelte so it can be unit tested — the RTF parser in
 * particular is a hand-rolled state machine with several interacting edge cases
 * (CP1252 escapes, \uN with \ucN fallback skipping, nested destinations), and a
 * regression in it is invisible until someone looks at a preview.
 */

/**
 * Windows-1252 code points for the bytes 0x80-0x9F — the only range where
 * CP1252 diverges from Latin-1. Ported from `cp1252_byte_to_char` in
 * `crates/copywraith-core/src/content.rs` so the two implementations agree.
 *
 * A table rather than the platform decoder, because that depends on the host's
 * ICU data: a Node build without full ICU silently decodes these bytes as
 * Latin-1, turning smart quotes and dashes into invisible C1 control
 * characters. That is exactly the corruption the decode exists to prevent, and
 * it reproduced on CI while passing locally.
 *
 * 0xFFFD marks the five positions unassigned in CP1252.
 */
const CP1252_HIGH: readonly number[] = [
	0x20ac, 0xfffd, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, // 0x80-0x87
	0x02c6, 0x2030, 0x0160, 0x2039, 0x0152, 0xfffd, 0x017d, 0xfffd, // 0x88-0x8F
	0xfffd, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, // 0x90-0x97
	0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0xfffd, 0x017e, 0x0178 // 0x98-0x9F
];

/** Render a code point, leaving invalid ones out rather than throwing. */
function codePointToString(codePoint: number): string {
	if (!Number.isFinite(codePoint) || codePoint < 0 || codePoint > 0x10ffff) return '';
	// Lone surrogates are not valid scalar values.
	if (codePoint >= 0xd800 && codePoint <= 0xdfff) return '';
	return String.fromCodePoint(codePoint);
}

/** Decode one Windows-1252 byte. Outside 0x80-0x9F, CP1252 is Latin-1. */
function cp1252ByteToChar(byte: number): string {
	if (byte >= 0x80 && byte <= 0x9f) {
		return String.fromCharCode(CP1252_HIGH[byte - 0x80]);
	}
	return String.fromCharCode(byte);
}

/** RTF header groups whose contents are markup, not document text. */
const RTF_SKIPPED_GROUPS = [
	'fonttbl',
	'colortbl',
	'expandedcolortbl',
	'stylesheet',
	'info',
	// An embedded picture's payload is hex-encoded image bytes. Without this
	// a rich-text copy containing an inline image previews as a wall of hex.
	'pict'
];

/**
 * Strip RTF markup to plain text with a single linear pass.
 *
 * The previous implementation stripped header groups with
 * `\{\\(?:fonttbl|...)[^}]*(?:\{[^}]*\}[^}]*)*\}`. Because `[^}]*` is greedy
 * but cannot cross a `}`, that pattern always ends at the *first* closing
 * brace, so it only ever removes the first nested group of a header. Real
 * word processors emit nested font and colour tables, and the remainder then
 * fell through to the blunt `\\[a-z]+\d*\s?` and `[{}]` passes. Observed
 * results on representative RTF:
 *
 *   {\fonttbl{\f0 Helvetica;}{\f1 Times New Roman;}}Actual text
 *     old -> "Times New Roman;Actual text"   (font name leaks into preview)
 *   Line one\par Line two
 *     old -> "Line oneLine two"              (paragraphs run together)
 *   Escaped \{braces\} here
 *     old -> "Escaped \\braces\\ here"       (escapes inverted)
 *   {\colortbl;...;}Body copy
 *     old -> "copy"                          ("Body" swallowed)
 *
 * Tracking brace depth handles arbitrary nesting, and a single pass with no
 * backtracking is O(n) by construction.
 */
export function rtfToPlainText(rtf: string): string {
	if (!rtf.trimStart().startsWith('{\\rtf')) return rtf;

	const length = rtf.length;
	let out = '';
	let depth = 0;
	// Depth of the header group currently being skipped, or -1 for none.
	let skipDepth = -1;
	// Fallback characters emitted after each \uN, set by \ucN. Default is 1.
	// \ucN is scoped to the group it appears in, so the value is saved on `{`
	// and restored on `}`. Word emits \uc0 inside field results and embedded
	// objects; leaking that outward disables fallback skipping for the rest of
	// the document and reintroduces duplicated characters.
	let ucSkip = 1;
	const ucStack: number[] = [];
	let i = 0;

	while (i < length) {
		const ch = rtf[i];

		if (ch === '{') {
			depth += 1;
			ucStack.push(ucSkip);
			if (skipDepth < 0 && rtf[i + 1] === '\\') {
				const head = rtf.slice(i + 2, i + 2 + 20);
				// `\*\` marks a destination the reader may ignore wholesale.
				if (head.startsWith('*\\') || RTF_SKIPPED_GROUPS.some((g) => head.startsWith(g))) {
					skipDepth = depth;
				}
			}
			i += 1;
			continue;
		}

		if (ch === '}') {
			if (skipDepth === depth) skipDepth = -1;
			// Malformed RTF can close more groups than it opened.
			depth = Math.max(0, depth - 1);
			const restored = ucStack.pop();
			ucSkip = restored === undefined ? 1 : restored;
			i += 1;
			continue;
		}

		if (skipDepth >= 0) {
			i += 1;
			continue;
		}

		if (ch === '\\') {
			i += 1;
			const next = rtf[i];
			if (next === undefined) break;

			// Escaped literals.
			if (next === '\\' || next === '{' || next === '}') {
				out += next;
				i += 1;
				continue;
			}
			// Single-character control symbols. These are not control *words*,
			// so they never reach the alphabetic scan below: `word` comes back
			// empty, nothing is consumed, and the symbol is then emitted as
			// literal text. \~ (non-breaking space) is common in Word output
			// and surfaced in previews as a stray "~".
			if (next === '~') {
				out += ' ';
				i += 1;
				continue;
			}
			// Non-breaking and optional hyphens carry no preview value.
			// (\- already happens to be eaten by the numeric-parameter scan,
			// but relying on that is accidental.)
			if (next === '_' || next === '-') {
				i += 1;
				continue;
			}
			// \'xx hex escape. RTF encodes these in the document codepage;
			// Windows-1252 is overwhelmingly the common case and is what the
			// Rust strip_rtf assumes too. Word and Outlook emit smart quotes,
			// em dashes, and accented letters this way constantly, so
			// dropping them (as the old regex did) visibly mangles previews.
			if (next === "'") {
				const hex = rtf.slice(i + 1, i + 3);
				if (/^[0-9a-f]{2}$/i.test(hex)) {
					out += cp1252ByteToChar(Number.parseInt(hex, 16));
				}
				i += 3;
				continue;
			}
			// A backslash before a newline is a line continuation. Consume the
			// whole CRLF pair — Windows RTF writers emit \r\n, and leaving the
			// \n behind turns into a stray space via the final whitespace
			// collapse, so output differed by the producer's platform.
			if (next === '\n' || next === '\r') {
				i += 1;
				if (next === '\r' && rtf[i] === '\n') i += 1;
				continue;
			}

			const wordStart = i;
			while (i < length && /[a-zA-Z]/.test(rtf[i])) i += 1;
			const word = rtf.slice(wordStart, i);

			// Optional numeric parameter, then the space delimiter.
			const paramStart = i;
			while (i < length && (rtf[i] === '-' || (rtf[i] >= '0' && rtf[i] <= '9'))) i += 1;
			const param = rtf.slice(paramStart, i);
			if (rtf[i] === ' ') i += 1;

			if (word === 'par' || word === 'line') out += '\n';
			else if (word === 'tab') out += '\t';
			else if (word === 'uc' && param) {
				// \uc0 means \uN carries no fallback at all; hardcoding a
				// skip of 1 would eat the following real character.
				const count = Number.parseInt(param, 10);
				ucSkip = Number.isFinite(count) ? Math.min(Math.max(count, 0), 8) : 1;
			} else if (word === 'u' && param) {
				// \uN is a signed 16-bit UTF-16 code unit; negatives wrap.
				const signed = Number.parseInt(param, 10);
				if (Number.isFinite(signed)) {
					const unit = signed < 0 ? signed + 65536 : signed;
					// Surrogate halves are emitted as-is; adjacent halves
					// recombine naturally in the resulting JS string.
					out += String.fromCharCode(unit);
				}
				// Skip the fallback characters emitted for readers that
				// cannot handle \u — otherwise they show as stray "?"s.
				//
				// A fallback unit is either a plain character or a whole
				// \'xx escape. Word and Outlook overwhelmingly emit the
				// latter (舗\'92), so treating the backslash as a stop
				// condition leaves the escape to be decoded by the main
				// loop, printing the same character twice.
				let remaining = ucSkip;
				while (remaining > 0 && i < length) {
					if (rtf[i] === '\\' && rtf[i + 1] === "'") {
						// \'xx counts as a single fallback character.
						i = Math.min(i + 4, length);
						remaining -= 1;
					} else if (rtf[i] !== '{' && rtf[i] !== '}' && rtf[i] !== '\\') {
						i += 1;
						remaining -= 1;
					} else {
						// A group delimiter or another control word: the
						// fallback run is over.
						break;
					}
				}
			}
			continue;
		}

		out += ch;
		i += 1;
	}

	// \par and \line emit \n above so that paragraph boundaries separate
	// words — without them "Line one\\par Line two" collapses to
	// "Line oneLine two". Flattening that \n to a space here is the desired
	// final form: the only consumer is the one-line .preview cell, which is
	// white-space: nowrap and would render a newline as a space regardless.
	return out.replace(/\s+/g, ' ').trim();
}

const HTML_ENTITIES: Readonly<Record<string, string>> = {
	nbsp: ' ', lt: '<', gt: '>', quot: '"', apos: "'", amp: '&'
};

export function htmlToPlainText(html: string): string {
	return html
		.replace(/<style[\s\S]*?<\/style>/gi, ' ')
		.replace(/<script[\s\S]*?<\/script>/gi, ' ')
		.replace(/<[^>]+>/g, ' ')
		// Decode each source entity once, including numeric ampersands.
		.replace(/&(nbsp|lt|gt|quot|apos|amp|#x[0-9a-f]{1,6}|#\d{1,7});/gi, (_, entity: string) => {
			const name = entity.toLowerCase();
			if (name.startsWith('#x')) return codePointToString(Number.parseInt(name.slice(2), 16));
			if (name.startsWith('#')) return codePointToString(Number.parseInt(name.slice(1), 10));
			return HTML_ENTITIES[name];
		})
		.replace(/\s+/g, ' ')
		.trim();
}
