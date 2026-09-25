/** Mirrors `CapturePauseStatus` in `src-tauri/src/models.rs`. */
export interface CapturePauseStatus {
	paused: boolean;
	/** When a timed pause ends (RFC 3339); null while paused means until resumed. */
	until: string | null;
}

export const NOT_PAUSED: CapturePauseStatus = { paused: false, until: null };

/** Pause lengths offered in the menu; null pauses until resumed. */
export const PAUSE_CHOICES: readonly { minutes: number | null; label: string }[] = [
	{ minutes: 5, label: 'Pause for 5 minutes' },
	{ minutes: 60, label: 'Pause for 1 hour' },
	{ minutes: null, label: 'Pause until resumed' }
];

/** Whether capture is paused at `nowMs`; a timed pause ends by itself. */
export function isPausedAt(status: CapturePauseStatus, nowMs: number): boolean {
	if (!status.paused) return false;
	if (status.until === null) return true;
	const until = new Date(status.until).getTime();
	return Number.isNaN(until) || nowMs < until;
}

/** Status-bar text: empty while capturing, otherwise how long the ghost sleeps. */
export function pauseLabel(status: CapturePauseStatus, nowMs: number): string {
	if (!isPausedAt(status, nowMs)) return '';
	if (status.until === null) return 'zzz Paused';

	const minutesLeft = Math.max(1, Math.ceil((new Date(status.until).getTime() - nowMs) / 60_000));
	return minutesLeft >= 60
		? `zzz Paused ${Math.floor(minutesLeft / 60)}h ${String(minutesLeft % 60).padStart(2, '0')}m`
		: `zzz Paused ${minutesLeft}m`;
}
