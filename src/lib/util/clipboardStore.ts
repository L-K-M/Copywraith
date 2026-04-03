import { writable, get } from 'svelte/store';
import { TauriService } from '$lib/tauri';
import { isMobile } from '$lib/util/platform';
import { notify } from '$lib/util/notifications';
import type { ClipboardEntry } from '$lib/types';

export const entries = writable<ClipboardEntry[]>([]);
export const isLoading = writable(false);
export const filterText = writable('');
export const starredOnly = writable(false);
export const selectedEntryId = writable<string | null>(null);

const LOAD_ENTRIES_TIMEOUT_MS = 10_000;

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let loadRequestId = 0;

async function getEntriesWithTimeout(options: {
	limit: number;
	offset: number;
	starred_only: boolean;
	search?: string;
}): Promise<ClipboardEntry[]> {
	let timeoutHandle: ReturnType<typeof setTimeout> | undefined;

	try {
		return await Promise.race([
			TauriService.getEntries(options),
			new Promise<ClipboardEntry[]>((_, reject) => {
				timeoutHandle = setTimeout(() => {
					reject(new Error(`Timed out loading clipboard entries after ${LOAD_ENTRIES_TIMEOUT_MS}ms`));
				}, LOAD_ENTRIES_TIMEOUT_MS);
			})
		]);
	} finally {
		if (timeoutHandle !== undefined) {
			clearTimeout(timeoutHandle);
		}
	}
}

function syncSelection(list: ClipboardEntry[], forceFirst = false) {
	const selectedId = get(selectedEntryId);

	if (list.length === 0) {
		selectedEntryId.set(null);
		return;
	}

	if (forceFirst) {
		selectedEntryId.set(list[0].id);
		return;
	}

	if (!selectedId || !list.some((entry) => entry.id === selectedId)) {
		selectedEntryId.set(list[0].id);
	}
}

export function selectFirstEntry(options?: { forceReselect?: boolean }) {
	const list = get(entries);
	if (list.length === 0) {
		selectedEntryId.set(null);
		return;
	}

	const firstId = list[0].id;
	if (options?.forceReselect && get(selectedEntryId) === firstId) {
		selectedEntryId.set(null);
		queueMicrotask(() => {
			const current = get(entries);
			if (current.length > 0) {
				selectedEntryId.set(current[0].id);
			}
		});
		return;
	}

	selectedEntryId.set(firstId);
}

export async function loadEntries(options?: { forceSelectFirst?: boolean }) {
	const requestId = ++loadRequestId;
	isLoading.set(true);

	try {
		const filter = get(filterText);
		const starred = get(starredOnly);
		const result = await getEntriesWithTimeout({
			limit: 100,
			offset: 0,
			starred_only: starred,
			search: filter || undefined
		});

		if (requestId !== loadRequestId) {
			return;
		}

		entries.set(result);
		syncSelection(result, options?.forceSelectFirst ?? false);
	} catch (e) {
		console.error('Failed to load entries:', e);
	} finally {
		if (requestId === loadRequestId) {
			isLoading.set(false);
		}
	}
}

export function debouncedLoad() {
	if (debounceTimer) clearTimeout(debounceTimer);
	debounceTimer = setTimeout(() => {
		selectedEntryId.set(null);
		loadEntries();
	}, 150);
}

export function selectEntry(id: string) {
	selectedEntryId.set(id);
}

export function moveSelection(delta: number) {
	const list = get(entries);
	if (list.length === 0) return;

	const selectedId = get(selectedEntryId);
	let index = list.findIndex((entry) => entry.id === selectedId);
	if (index === -1) index = 0;

	const nextIndex = Math.max(0, Math.min(list.length - 1, index + delta));
	selectedEntryId.set(list[nextIndex].id);
}

export async function pasteSelectedEntry() {
	const selectedId = get(selectedEntryId);
	if (!selectedId) return;
	await pasteEntry(selectedId);
}

export async function toggleStar(id: string) {
	try {
		const newStarred = await TauriService.toggleStar(id);
		entries.update((list) =>
			list.map((e) => (e.id === id ? { ...e, starred: newStarred } : e))
		);
	} catch (e) {
		console.error('Failed to toggle star:', e);
	}
}

export async function deleteEntry(id: string) {
	try {
		await TauriService.deleteEntry(id);
		entries.update((list) => {
			const updated = list.filter((entry) => entry.id !== id);
			syncSelection(updated);
			return updated;
		});
	} catch (e) {
		console.error('Failed to delete entry:', e);
	}
}

export async function pasteEntry(id: string) {
	try {
		await TauriService.pasteEntry(id);
		// On mobile, pasting just copies to clipboard — show feedback
		if (get(isMobile)) {
			notify('success', 'Copied to clipboard');
		}
	} catch (e) {
		console.error('Failed to paste entry:', e);
	}
}

export async function pasteEntryPlaintext(id: string) {
	try {
		await TauriService.pasteEntryPlaintext(id);
		if (get(isMobile)) {
			notify('success', 'Copied as plaintext');
		}
	} catch (e) {
		console.error('Failed to paste entry as plaintext:', e);
	}
}
