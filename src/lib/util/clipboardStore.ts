import { writable, get } from 'svelte/store';
import { TauriService } from '$lib/tauri';
import { isMobile } from '$lib/util/platform';
import { notify } from '$lib/util/notifications';
import type { ClipboardEntry } from '$lib/types';

export const entries = writable<ClipboardEntry[]>([]);
export const isLoading = writable(false);
export const isLoadingMore = writable(false);
export const hasMoreEntries = writable(false);
export const filterText = writable('');
export const starredOnly = writable(false);
export const selectedEntryId = writable<string | null>(null);

const PAGE_SIZE = 100;
const LOAD_ENTRIES_TIMEOUT_MS = 10_000;
const ERROR_NOTIFY_DEBOUNCE_MS = 5_000;

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let loadRequestId = 0;
let lastLoadErrorAt = 0;
let nextOffset = 0;

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
	isLoadingMore.set(false);
	hasMoreEntries.set(false);

	try {
		const filter = get(filterText);
		const starred = get(starredOnly);
		const result = await getEntriesWithTimeout({
			limit: PAGE_SIZE,
			offset: 0,
			starred_only: starred,
			search: filter || undefined
		});

		if (requestId !== loadRequestId) {
			return;
		}

		nextOffset = result.length;
		entries.set(result);
		hasMoreEntries.set(result.length === PAGE_SIZE);
		syncSelection(result, options?.forceSelectFirst ?? false);
	} catch (e) {
		console.error('Failed to load entries:', e);
		const now = Date.now();
		if (now - lastLoadErrorAt > ERROR_NOTIFY_DEBOUNCE_MS) {
			lastLoadErrorAt = now;
			notify('error', 'Failed to load clipboard entries');
		}
	} finally {
		if (requestId === loadRequestId) {
			isLoading.set(false);
		}
	}
}

export async function loadMoreEntries() {
	if (get(isLoading) || get(isLoadingMore) || !get(hasMoreEntries)) {
		return;
	}

	const requestId = loadRequestId;
	const offset = nextOffset;
	isLoadingMore.set(true);

	try {
		const filter = get(filterText);
		const starred = get(starredOnly);
		const result = await getEntriesWithTimeout({
			limit: PAGE_SIZE,
			offset,
			starred_only: starred,
			search: filter || undefined
		});

		if (requestId !== loadRequestId) {
			return;
		}

		nextOffset += result.length;
		hasMoreEntries.set(result.length === PAGE_SIZE);

		entries.update((list) => {
			const existingIds = new Set(list.map((entry) => entry.id));
			const appended = result.filter((entry) => !existingIds.has(entry.id));
			const updated = [...list, ...appended];
			syncSelection(updated);
			return updated;
		});
	} catch (e) {
		console.error('Failed to load more entries:', e);
		const now = Date.now();
		if (now - lastLoadErrorAt > ERROR_NOTIFY_DEBOUNCE_MS) {
			lastLoadErrorAt = now;
			notify('error', 'Failed to load more clipboard entries');
		}
	} finally {
		if (requestId === loadRequestId) {
			isLoadingMore.set(false);
		}
	}
}

export function debouncedLoad() {
	if (debounceTimer) clearTimeout(debounceTimer);
	// Invalidate the visible selection and any in-flight response as soon as
	// the query changes. Otherwise Enter can act on the previous result set
	// during the debounce window.
	loadRequestId += 1;
	selectedEntryId.set(null);
	isLoading.set(true);
	isLoadingMore.set(false);
	hasMoreEntries.set(false);
	debounceTimer = setTimeout(() => {
		loadEntries();
	}, 150);
}

export function selectEntry(id: string) {
	selectedEntryId.set(id);
}

export function moveSelection(delta: number) {
	if (get(isLoading)) return;

	const list = get(entries);
	if (list.length === 0) return;

	const selectedId = get(selectedEntryId);
	let index = list.findIndex((entry) => entry.id === selectedId);
	if (index === -1) index = 0;

	const nextIndex = Math.max(0, Math.min(list.length - 1, index + delta));
	selectedEntryId.set(list[nextIndex].id);
}

export async function pasteSelectedEntry() {
	if (get(isLoading)) return;

	const selectedId = get(selectedEntryId);
	if (!selectedId) return;
	await pasteEntry(selectedId);
}

export async function toggleStar(id: string) {
	try {
		const newStarred = await TauriService.toggleStar(id);
		entries.update((list) => {
			const updated =
				get(starredOnly) && !newStarred
					? list.filter((entry) => entry.id !== id)
					: list.map((entry) =>
							entry.id === id ? { ...entry, starred: newStarred } : entry
						);
			nextOffset = Math.min(nextOffset, updated.length);
			syncSelection(updated);
			return updated;
		});
	} catch (e) {
		console.error('Failed to toggle star:', e);
		notify('error', 'Failed to update starred state');
	}
}

export async function deleteEntry(id: string) {
	try {
		await TauriService.deleteEntry(id);
		entries.update((list) => {
			const updated = list.filter((entry) => entry.id !== id);
			nextOffset = Math.min(nextOffset, updated.length);
			syncSelection(updated);
			return updated;
		});
	} catch (e) {
		console.error('Failed to delete entry:', e);
		notify('error', 'Failed to delete entry');
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
		notify('error', e instanceof Error ? e.message : String(e));
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
		notify('error', e instanceof Error ? e.message : String(e));
	}
}
