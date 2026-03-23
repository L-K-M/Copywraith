import { writable, get } from 'svelte/store';
import { TauriService } from '$lib/tauri';
import type { ClipboardEntry } from '$lib/types';

export const entries = writable<ClipboardEntry[]>([]);
export const isLoading = writable(false);
export const filterText = writable('');
export const starredOnly = writable(false);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

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
	} catch (e) {
		console.error('Failed to load entries:', e);
	} finally {
		isLoading.set(false);
	}
}

export function debouncedLoad() {
	if (debounceTimer) clearTimeout(debounceTimer);
	debounceTimer = setTimeout(() => {
		loadEntries();
	}, 150);
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
		entries.update((list) => list.filter((e) => e.id !== id));
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
