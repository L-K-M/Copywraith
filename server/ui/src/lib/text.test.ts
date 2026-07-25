import { describe, expect, it } from 'vitest';

import { htmlToPlainText, rtfToPlainText } from './text';

// `String.raw` keeps RTF backslashes readable — without it every control word
// needs doubling and the tests stop resembling the input they describe.
const rtf = String.raw;

describe('rtfToPlainText', () => {
	it('passes non-RTF input straight through', () => {
		expect(rtfToPlainText('not rtf at all')).toBe('not rtf at all');
		expect(rtfToPlainText('')).toBe('');
	});

	it('extracts body text', () => {
		expect(rtfToPlainText(rtf`{\rtf1\ansi Hello world}`)).toBe('Hello world');
	});

	// The four cases below are the regressions that motivated replacing the
	// regex-chain implementation. Each records the output it used to produce.
	it('strips nested font tables instead of leaking font names', () => {
		// old: "Times New Roman;Actual text"
		expect(
			rtfToPlainText(
				rtf`{\rtf1\ansi{\fonttbl{\f0\fnil Helvetica;}{\f1\fswiss Times New Roman;}}\f0 Actual text}`
			)
		).toBe('Actual text');
	});

	it('strips colour tables without swallowing the text after them', () => {
		// old: "copy" — the leading "Body" was consumed
		expect(
			rtfToPlainText(rtf`{\rtf1\ansi{\colortbl;\red255\green0\blue0;}Body copy}`)
		).toBe('Body copy');
	});

	it('separates paragraphs instead of joining the words', () => {
		// old: "Line oneLine two"
		expect(rtfToPlainText(rtf`{\rtf1\ansi Line one\par Line two}`)).toBe('Line one Line two');
		expect(rtfToPlainText(rtf`{\rtf1\ansi a\line b}`)).toBe('a b');
		expect(rtfToPlainText(rtf`{\rtf1\ansi a\tab b}`)).toBe('a b');
	});

	it('preserves escaped literals instead of inverting them', () => {
		// old: "Escaped \\braces\\ here"
		expect(rtfToPlainText(rtf`{\rtf1\ansi Escaped \{braces\} here}`)).toBe(
			'Escaped {braces} here'
		);
		expect(rtfToPlainText(rtf`{\rtf1\ansi back\\slash}`)).toBe('back\\slash');
	});

	describe('CP1252 hex escapes', () => {
		it('decodes the 0x80-0x9F range rather than emitting C1 controls', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi It\'92s \'93quoted\'94}`)).toBe('It’s “quoted”');
			expect(rtfToPlainText(rtf`{\rtf1\ansi a\'97b}`)).toBe('a—b');
		});

		it('decodes the Latin-1 range', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi caf\'e9 na\'efve}`)).toBe('café naïve');
		});

		// Pins the whole divergent range. This started as a TextDecoder call,
		// which depends on the host's ICU data — a Node build without full ICU
		// decodes these as Latin-1 and yields invisible C1 control characters.
		// That passed locally and failed on CI, so the mapping is now explicit.
		it('maps every byte of 0x80-0x9F, never to a C1 control', () => {
			const expected: Record<number, string> = {
				0x80: '€', 0x82: '‚', 0x83: 'ƒ', 0x84: '„', 0x85: '…', 0x86: '†',
				0x87: '‡', 0x88: 'ˆ', 0x89: '‰', 0x8a: 'Š', 0x8b: '‹', 0x8c: 'Œ',
				0x8e: 'Ž', 0x91: '‘', 0x92: '’', 0x93: '“', 0x94: '”', 0x95: '•',
				0x96: '–', 0x97: '—', 0x98: '˜', 0x99: '™', 0x9a: 'š', 0x9b: '›',
				0x9c: 'œ', 0x9e: 'ž', 0x9f: 'Ÿ'
			};

			for (const [raw, char] of Object.entries(expected)) {
				const byte = Number(raw);
				const hex = byte.toString(16).padStart(2, '0');
				expect(rtfToPlainText(`{\\rtf1\\ansi a\\'${hex}b`)).toBe(`a${char}b`);
			}

			// The five unassigned positions become U+FFFD, not a C1 control.
			for (const byte of [0x81, 0x8d, 0x8f, 0x90, 0x9d]) {
				const hex = byte.toString(16).padStart(2, '0');
				expect(rtfToPlainText(`{\\rtf1\\ansi a\\'${hex}b`)).toBe('a�b');
			}
		});

		it('ignores a malformed escape rather than emitting garbage', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi a\'zzb}`)).toBe('ab');
		});
	});

	describe('\\uN unicode escapes', () => {
		it('decodes the code unit and drops the fallback character', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi It\u8217?s here}`)).toBe('It’s here');
		});

		it('wraps negative values and recombines surrogate pairs', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi \u-10179?\u-8704?!}`)).toBe('\u{1F600}!');
		});

		// Regression: decoding \'xx made the fallback skip wrong, because the
		// skip loop stopped at any backslash. Word emits the fallback *as* a
		// \'xx escape, so it fell through and printed the character twice.
		it('consumes a \\_xx hex escape used as the fallback', () => {
			// Lifted from copywraith-core's test_strip_rtf_unicode_hex_fallback.
			expect(rtfToPlainText(rtf`{\rtf1\ansi caf\u233\'e9 au lait}`)).toBe('café au lait');
			expect(rtfToPlainText(rtf`{\rtf1\ansi\uc1 \u8217\'92Hello}`)).toBe('’Hello');
		});

		it('honours \\uc0 (no fallback) and \\uc2 (two)', () => {
			// Matches copywraith-core: the space after \u233 is the control-word
			// delimiter, not text, so the words run together under \uc0.
			expect(rtfToPlainText(rtf`{\rtf1\ansi\uc0 caf\u233 done}`)).toBe('cafédone');
			expect(rtfToPlainText(rtf`{\rtf1\ansi\uc2 caf\u233 ?? done}`)).toBe('café done');
			expect(rtfToPlainText(rtf`{\rtf1\ansi\uc2 \u8217\'92\'92Hi}`)).toBe('’Hi');
		});

		it('scopes \\ucN to its group', () => {
			// A nested \uc0 must not disable fallback skipping for the rest of
			// the document — that reintroduces the duplicated-character bug.
			expect(rtfToPlainText(rtf`{\rtf1\ansi{\uc0 a\u233 b}caf\u233\'e9 x}`)).toBe(
				'aébcafé x'
			);
		});
	});

	describe('control symbols', () => {
		it('renders \\~ as a space rather than a literal tilde', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi Hello\~world}`)).toBe('Hello world');
		});

		it('drops non-breaking and optional hyphens', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi co\_op}`)).toBe('coop');
			expect(rtfToPlainText(rtf`{\rtf1\ansi hy\-phen}`)).toBe('hyphen');
		});
	});

	describe('malformed input', () => {
		it('survives more closing braces than opening ones', () => {
			expect(rtfToPlainText(rtf`{\rtf1\ansi Hello}}}} world}`)).toBe('Hello world');
		});

		it('survives a trailing backslash', () => {
			// Built with a quoted string rather than a template: inside a
			// template literal a trailing `\` escapes the closing backtick.
			expect(rtfToPlainText('{\\rtf1\\ansi trailing\\')).toBe('trailing');
		});
	});

	it('stays linear on a large nested font table', () => {
		// The regex it replaced was suspected of backtracking here. It was not,
		// but the guard is cheap and the shape is what real documents look like.
		const input = rtf`{\rtf1\ansi{\fonttbl` + rtf`{\f0\fnil Arial;}`.repeat(5000) + '}Text';
		const started = Date.now();
		expect(rtfToPlainText(input)).toBe('Text');
		expect(Date.now() - started).toBeLessThan(1000);
	});
});

describe('htmlToPlainText', () => {
	it('strips tags and collapses whitespace', () => {
		expect(htmlToPlainText('<p>Hello <b>world</b></p>')).toBe('Hello world');
	});

	it('drops style and script contents', () => {
		expect(htmlToPlainText('<style>p{color:red}</style><p>visible</p>')).toBe('visible');
		expect(htmlToPlainText('<script>alert(1)</script><p>visible</p>')).toBe('visible');
	});

	it('decodes the common named entities', () => {
		expect(htmlToPlainText('a &amp; b &lt; c &gt; d &quot;e&quot; &#39;f&#39;')).toBe(
			'a & b < c > d "e" \'f\''
		);
	});
});
