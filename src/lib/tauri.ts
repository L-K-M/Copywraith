import { addPluginListener, invoke, type PluginListener } from '@tauri-apps/api/core';
import type { ClipboardEntry, Settings } from './types';
import type { SyncEndpointStatusInput } from './util/syncStatusStore';

export interface SyncNowResult {
	pulled: number;
	endpoint_status: SyncEndpointStatusInput;
}

export interface ImportPendingSharesResult {
	imported: number;
	skipped: number;
}

export interface PendingSharesStatus {
	pending: boolean;
	staged: boolean;
}

export interface ShizukuClipboardStatus {
	state: string;
	message: string;
	available: boolean;
	enabled: boolean;
	listening: boolean;
	started?: boolean | null;
	backend_uid?: number | null;
	last_clipboard_text_at?: number | null;
}

export interface ShizukuClipboardStagedEvent {
	captured_at: number;
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

	/** Import Android share-sheet payloads staged by the native Activity. */
	static async importPendingShares(): Promise<ImportPendingSharesResult> {
		return await invoke('import_pending_shares');
	}

	/** Check whether Android share-sheet payloads are waiting to import. */
	static async hasPendingShares(): Promise<PendingSharesStatus> {
		return await invoke('has_pending_shares');
	}

	/** Push local changes and pull the latest entries from the server now. */
	static async syncNow(): Promise<SyncNowResult> {
		return await invoke('sync_now');
	}

	/** Reset pull cursor so the next sync re-scans server entries. */
	static async resetSyncCursor(): Promise<{ reset: boolean }> {
		return await invoke('reset_sync_cursor');
	}

	/** Get Android Shizuku clipboard listener status. */
	static async shizukuClipboardStatus(): Promise<ShizukuClipboardStatus> {
		return await invoke('shizuku_clipboard_status');
	}

	/** Enable or disable Android Shizuku clipboard listener. */
	static async setShizukuClipboardEnabled(enabled: boolean): Promise<ShizukuClipboardStatus> {
		return await invoke('set_shizuku_clipboard_enabled', { enabled });
	}

	/** Listen for Android Shizuku-staged clipboard entries. */
	static async onShizukuClipboardStaged(
		callback: (event: ShizukuClipboardStagedEvent) => void
	): Promise<PluginListener> {
		return await addPluginListener(
			'copywraith-share-target',
			'shizuku-clipboard-staged',
			callback
		);
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
