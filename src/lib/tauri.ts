import { invoke } from '@tauri-apps/api/core';
import type { ClipboardEntry, Settings } from './types';
import type { SyncEndpointStatusInput } from './util/syncStatusStore';

export interface SyncNowResult {
	pulled: number;
	endpoint_status: SyncEndpointStatusInput;
}

export class TauriService {
	static async getEntries(options?: {
		limit?: number;
		offset?: number;
		starred_only?: boolean;
		search?: string;
	}): Promise<ClipboardEntry[]> {
		return await invoke('get_entries', {
			limit: options?.limit ?? 50,
			offset: options?.offset ?? 0,
			starredOnly: options?.starred_only ?? false,
			search: options?.search ?? null
		});
	}

	static async toggleStar(id: string): Promise<boolean> {
		return await invoke('toggle_star', { id });
	}

	static async deleteEntry(id: string): Promise<boolean> {
		return await invoke('delete_entry', { id });
	}

	static async getEntryImage(id: string): Promise<string | null> {
		return await invoke('get_entry_image', { id });
	}

	static async pasteEntry(id: string): Promise<void> {
		await invoke('paste_entry', { id });
	}

	static async pasteEntryPlaintext(id: string): Promise<void> {
		await invoke('paste_entry_plaintext', { id });
	}

	static async getSettings(): Promise<Settings> {
		return await invoke('get_settings');
	}

	static async updateSettings(settings: Settings): Promise<void> {
		await invoke('update_settings', { settings });
	}

	static async reregisterShortcuts(): Promise<void> {
		await invoke('reregister_shortcuts');
	}

	/** Read the current system clipboard and save it as a new entry (mobile). */
	static async captureClipboard(): Promise<boolean> {
		return await invoke('capture_clipboard');
	}

	/** Push local changes and pull the latest entries from the server now. */
	static async syncNow(): Promise<SyncNowResult> {
		return await invoke('sync_now');
	}

	/** Returns the current platform: "android", "ios", "macos", "windows", "linux". */
	static async getPlatform(): Promise<string> {
		return await invoke('get_platform');
	}

	/** Hides the popup window (and NSPanel on macOS when enabled). */
	static async hidePopup(): Promise<void> {
		await invoke('hide_popup');
	}
}
