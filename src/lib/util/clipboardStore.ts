import { writable, get } from 'svelte/store';
import { TauriService } from '$lib/tauri';
import type { ClipboardEntry } from '$lib/types';

export const entries = writable<ClipboardEntry[]>([]);
export const isLoading = writable(false);
export const filterText = writable('');
export const starredOnly = writable(false);
export const selectedEntryId = writable<string | null>(null);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

function syncSelection(list: ClipboardEntry[]) {
	const selectedId = get(selectedEntryId);

	if (list.length === 0) {
		selectedEntryId.set(null);
		return;
	}

	if (!selectedId || !list.some((entry) => entry.id === selectedId)) {
		selectedEntryId.set(list[0].id);
	}
}

export async function loadEntries() {
	isLoading.set(true);
	try {
		const filter = get(filterText);
		const starred = get(starredOnly);
		const result = await TauriService.getEntries({
			limit: 100,
			offset: 0,
			starred_only: starred,
			search: filter || undefined
		});
		entries.set(result);
		syncSelection(result);
	} catch (e) {
		console.error('Failed to load entries:', e);
	} finally {
		isLoading.set(false);
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
	} catch (e) {
		console.error('Failed to paste entry:', e);
	}
}

export async function pasteEntryPlaintext(id: string) {
	try {
		await TauriService.pasteEntryPlaintext(id);
	} catch (e) {
		console.error('Failed to paste entry as plaintext:', e);
	}
}
