import { writable } from 'svelte/store';

export type SyncEndpointState = 'checking' | 'disabled' | 'online' | 'unreachable';

export interface SyncEndpointStatus {
	state: SyncEndpointState;
	role: string | null;
	url: string | null;
}

export const syncEndpointStatus = writable<SyncEndpointStatus>({
	state: 'checking',
	role: null,
	url: null
});
