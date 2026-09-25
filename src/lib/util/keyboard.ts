/**
 * Keyboard shortcuts for the desktop popup, as a pure mapping from a key event
 * to an action so the rules can be tested without a WebView.
 *
 * "Mod" is Cmd on macOS and Ctrl elsewhere.
 */

export type ShortcutAction =
	| { type: 'quick-paste'; index: number }
	| { type: 'paste-plaintext' }
	| { type: 'toggle-star' }
	| { type: 'delete' }
	| { type: 'preview' }
	| { type: 'move'; delta: number }
	| { type: 'select-edge'; edge: 'first' | 'last' }
	| { type: 'focus-filter' };

export interface ShortcutKey {
	key: string;
	metaKey: boolean;
	ctrlKey: boolean;
	shiftKey: boolean;
	altKey: boolean;
	/** The OS is auto-repeating a held key. */
	repeat?: boolean;
}

export interface ShortcutContext {
	platform: string;
	/** Focus is in a text field, so plain keys edit text. */
	inTextField: boolean;
	/** The filter field has no text, so text-editing combos have nothing to act on. */
	filterEmpty: boolean;
}

/** Rows a PageUp/PageDown jump moves the selection by. */
export const PAGE_JUMP = 10;

/** Number of rows reachable with Mod+1..9. */
export const QUICK_PASTE_SLOTS = 9;

export function isModKey(key: string, platform: string): boolean {
	return platform === 'macos' ? key === 'Meta' : key === 'Control';
}

export function resolveShortcut(event: ShortcutKey, context: ShortcutContext): ShortcutAction | null {
	const action = resolveKey(event, context);
	// A held chord must not delete, star or paste over and over; only moving
	// the selection is meant to repeat.
	if (event.repeat && action?.type !== 'move') return null;
	return action;
}

function resolveKey(event: ShortcutKey, context: ShortcutContext): ShortcutAction | null {
	const mod = context.platform === 'macos' ? event.metaKey : event.ctrlKey;

	// Option/Alt+Enter matches Option/Alt+click: paste as plain text.
	if (event.key === 'Enter' && event.altKey && !mod && !event.shiftKey) {
		return { type: 'paste-plaintext' };
	}
	// Otherwise keep AltGr/Option layouts free to type characters.
	if (event.altKey) return null;

	if (mod && !event.shiftKey) {
		const slot = /^[1-9]$/.test(event.key) ? Number(event.key) : 0;
		if (slot >= 1 && slot <= QUICK_PASTE_SLOTS) return { type: 'quick-paste', index: slot - 1 };

		switch (event.key.toLowerCase()) {
			case 's':
				return { type: 'toggle-star' };
			case 'y':
				// Quick Look's shortcut in Finder.
				return { type: 'preview' };
			case 'f':
				return { type: 'focus-filter' };
		}

		// In a filter with text, Mod+Backspace deletes that text instead.
		if (
			(event.key === 'Backspace' || event.key === 'Delete') &&
			(!context.inTextField || context.filterEmpty)
		) {
			return { type: 'delete' };
		}
		return null;
	}

	if (mod) return null;

	if (event.key === 'Enter' && event.shiftKey) return { type: 'paste-plaintext' };
	if (event.shiftKey) return null;

	if (event.key === 'PageDown') return { type: 'move', delta: PAGE_JUMP };
	if (event.key === 'PageUp') return { type: 'move', delta: -PAGE_JUMP };

	// Home and End move the caret inside a text field.
	if (!context.inTextField) {
		if (event.key === 'Home') return { type: 'select-edge', edge: 'first' };
		if (event.key === 'End') return { type: 'select-edge', edge: 'last' };
	}

	return null;
}
