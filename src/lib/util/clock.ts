import { readable } from 'svelte/store';

/** How often the shared clock advances. */
const TICK_MS = 30_000;

/**
 * A single clock shared by every component that renders a relative time.
 *
 * Relative labels ("now", "5m", "3h") used to be computed once at render and
 * then never again, so a row that said "now" kept saying "now" until something
 * else forced a re-render. Subscribing to this store makes those labels age
 * correctly.
 *
 * One interval serves the whole list rather than one timer per row, and
 * `readable`'s start/stop contract means the interval only runs while something
 * is actually subscribed — it stops when the popup is hidden or destroyed.
 */
export const now = readable(Date.now(), (set) => {
	const handle = setInterval(() => set(Date.now()), TICK_MS);
	return () => clearInterval(handle);
});

/**
 * Format `dateStr` as a short age relative to `nowMs`.
 *
 * Falls back to a locale date for anything older than 30 days, and returns an
 * em dash rather than "NaN" for an unparseable timestamp.
 */
export function formatRelativeTime(dateStr: string, nowMs: number): string {
	const timestamp = new Date(dateStr).getTime();
	if (Number.isNaN(timestamp)) return '—';

	// Clock skew between devices can put a synced entry slightly in the future.
	const diffSec = Math.max(0, Math.floor((nowMs - timestamp) / 1000));
	const diffMin = Math.floor(diffSec / 60);
	const diffHour = Math.floor(diffMin / 60);
	const diffDay = Math.floor(diffHour / 24);

	if (diffSec < 60) return 'now';
	if (diffMin < 60) return `${diffMin}m`;
	if (diffHour < 24) return `${diffHour}h`;
	if (diffDay < 30) return `${diffDay}d`;
	return new Date(timestamp).toLocaleDateString();
}

/**
 * Guess an image MIME type from the leading bytes of a base64 payload.
 *
 * Four base64 characters encode exactly three bytes, so a prefix comparison
 * reads the magic number without decoding anything. Data URLs previously
 * hardcoded `image/png` for every format.
 */
export function imageMimeFromBase64(base64: string): string {
	if (base64.startsWith('iVBORw0KGgo')) return 'image/png';
	if (base64.startsWith('/9j/')) return 'image/jpeg';
	if (base64.startsWith('R0lGOD')) return 'image/gif';
	if (base64.startsWith('UklGR')) return 'image/webp';
	if (base64.startsWith('Qk')) return 'image/bmp';
	return 'application/octet-stream';
}
