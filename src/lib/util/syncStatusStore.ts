import { writable } from 'svelte/store';

export type SyncEndpointState = 'checking' | 'disabled' | 'online' | 'unreachable';

export interface SyncEndpointStatus {
	state: SyncEndpointState;
	role: string | null;
	url: string | null;
	message: string | null;
	checked_at: string | null;
}

export interface SyncEndpointStatusInput {
	state: string;
	role?: string | null;
	url?: string | null;
	message?: string | null;
	checked_at?: string | null;
}

function normalizeState(state: string): SyncEndpointState {
	return state === 'online' ||
		state === 'disabled' ||
		state === 'checking' ||
		state === 'unreachable'
		? state
		: 'unreachable';
}

function defaultMessage(state: SyncEndpointState): string {
	if (state === 'checking') return 'A sync check is running or waiting for a backend response.';
	if (state === 'disabled') return 'No server URL is configured in Settings.';
	if (state === 'online') return 'The last sync check reached a server endpoint.';
	return 'No configured sync endpoint responded successfully.';
}

export function normalizeSyncEndpointStatus(payload: SyncEndpointStatusInput): SyncEndpointStatus {
	const state = normalizeState(payload.state);

	return {
		state,
		role: payload.role ?? null,
		url: payload.url ?? null,
		message: payload.message ?? defaultMessage(state),
		checked_at: payload.checked_at ?? new Date().toISOString()
	};
}

export const syncEndpointStatus = writable<SyncEndpointStatus>({
	state: 'checking',
	role: null,
	url: null,
	message: 'Waiting for the first sync status update.',
	checked_at: null
});

let checkingWatchdog: ReturnType<typeof setTimeout> | null = null;

export function setSyncEndpointStatus(payload: SyncEndpointStatusInput) {
	const status = normalizeSyncEndpointStatus(payload);
	syncEndpointStatus.set(status);

	if (checkingWatchdog) {
		clearTimeout(checkingWatchdog);
		checkingWatchdog = null;
	}

	if (status.state === 'checking') {
		checkingWatchdog = setTimeout(() => {
			syncEndpointStatus.update((current) => {
				if (current.state !== 'checking') return current;

				return {
					...current,
					state: 'unreachable',
					message:
						'Sync did not report completion within 45 seconds. The request may be stuck in the network stack or while pushing unsynced entries.',
					checked_at: new Date().toISOString()
				};
			});
			checkingWatchdog = null;
		}, 45000);
	}
}
