<script lang="ts">
	import { Button, MovableDialog } from '@lkmc/system7-ui';
	import { TauriService } from '$lib/tauri';
	import { isMobile } from '$lib/util/platform';
	import { notify } from '$lib/util/notifications';
	import { onMount } from 'svelte';

	let { onclose }: { onclose: () => void } = $props();

	let primaryServerUrl = $state('');
	let fallbackServerUrl = $state('');
	let apiKey = $state('');
	let shortcutTogglePopup = $state('CmdOrCtrl+Shift+V');
	let shortcutStarredPopup = $state('CmdOrCtrl+Shift+B');
	let shortcutPastePlaintext = $state('CmdOrCtrl+Shift+Alt+V');
	let shizukuClipboardEnabled = $state(false);
	let shizukuState = $state('unknown');
	let shizukuMessage = $state('Shizuku status has not been checked yet.');
	let shizukuBusy = $state(false);

	onMount(async () => {
		try {
			const settings = await TauriService.getSettings();
			primaryServerUrl = settings.server_url_primary;
			fallbackServerUrl = settings.server_url_fallback;
			apiKey = settings.api_key;
			shortcutTogglePopup = settings.shortcut_toggle_popup;
			shortcutStarredPopup = settings.shortcut_starred_popup;
			shortcutPastePlaintext = settings.shortcut_paste_plaintext;
			shizukuClipboardEnabled = settings.shizuku_clipboard_enabled;
			if ($isMobile) {
				void refreshShizukuStatus();
			}
		} catch (e) {
			console.error('Failed to load settings:', e);
			notify('error', 'Failed to load settings');
		}
	});

	async function handleSave() {
		try {
			await TauriService.updateSettings({
				server_url_primary: normalizeServerUrl(primaryServerUrl),
				server_url_fallback: normalizeServerUrl(fallbackServerUrl),
				api_key: apiKey,
				shortcut_toggle_popup: shortcutTogglePopup,
				shortcut_starred_popup: shortcutStarredPopup,
				shortcut_paste_plaintext: shortcutPastePlaintext,
				shizuku_clipboard_enabled: shizukuClipboardEnabled
			});
			if (!$isMobile) {
				await TauriService.reregisterShortcuts();
			}
			void TauriService.syncNow().catch((e) => {
				console.error('Failed to refresh sync status after saving settings:', e);
			});
			notify('success', 'Settings saved');
			onclose();
		} catch (e) {
			notify('error', `Failed to save settings: ${e}`);
		}
	}

	async function refreshShizukuStatus() {
		try {
			const status = await TauriService.shizukuClipboardStatus();
			shizukuState = status.state;
			shizukuMessage = formatShizukuStatus(status);
			shizukuClipboardEnabled = status.enabled || shizukuClipboardEnabled;
		} catch (e) {
			shizukuState = 'unavailable';
			shizukuMessage = `Shizuku status unavailable: ${e}`;
		}
	}

	async function handleToggleShizukuClipboard() {
		if (shizukuBusy) return;
		shizukuBusy = true;
		const nextEnabled = !shizukuClipboardEnabled;
		try {
			const status = await TauriService.setShizukuClipboardEnabled(nextEnabled);
			shizukuClipboardEnabled = status.enabled;
			shizukuState = status.state;
			shizukuMessage = formatShizukuStatus(status);
			if (status.listening) {
				notify('success', 'Shizuku clipboard listener enabled');
			} else if (!nextEnabled) {
				notify('success', 'Shizuku clipboard listener disabled');
			} else {
				notify('info', status.message);
			}
		} catch (e) {
			notify('error', `Failed to update Shizuku listener: ${e}`);
		} finally {
			shizukuBusy = false;
		}
	}

	function formatShizukuStatus(status: Awaited<ReturnType<typeof TauriService.shizukuClipboardStatus>>) {
		const backend =
			status.backend_uid === 0
				? 'root'
				: status.backend_uid === 2000
					? 'ADB shell'
					: status.backend_uid != null
						? `UID ${status.backend_uid}`
						: null;
		return backend ? `${status.message} Backend: ${backend}.` : status.message;
	}

	async function handleResetSyncCursor() {
		try {
			await TauriService.resetSyncCursor();
			await TauriService.syncNow();
			notify('success', 'Sync cursor reset. Re-scanning server entries.');
		} catch (e) {
			notify('error', `Failed to reset sync cursor: ${e}`);
		}
	}

	function normalizeServerUrl(url: string) {
		return url.trim().replace(/\/+$/, '');
	}
</script>

{#snippet settingsForm()}
	<div class="settings-form">
		<div class="s7-form-group">
			<label for="primary-server-url">Local Server URL</label>
			<input
				id="primary-server-url"
				type="url"
				class="s7-input"
				placeholder="http://192.168.1.5:3742"
				bind:value={primaryServerUrl}
			/>
			<div class="field-hint">Used first. This can be your local Wi-Fi address.</div>
		</div>

		<div class="s7-form-group">
			<label for="fallback-server-url">VPN Server URL</label>
			<input
				id="fallback-server-url"
				type="url"
				class="s7-input"
				placeholder="http://100.64.0.10:3742"
				bind:value={fallbackServerUrl}
			/>
			<div class="field-hint">
				Used when the local server cannot be reached. This can be your Tailscale/VPN address.
			</div>
		</div>

		<div class="s7-form-group">
			<label for="server-password">Server Password</label>
			<input
				id="server-password"
				type="password"
				class="s7-input"
				placeholder="Password from admin UI"
				bind:value={apiKey}
			/>
			<div class="field-hint">
				Use the same password configured on the server admin UI. Copywraith sends it as an
				Authorization: Bearer header.
			</div>
		</div>

		{#if $isMobile}
			<div class="section-divider"></div>
			<div class="section-label">Advanced Android Clipboard</div>
			<div class="field-hint">
				Optional Shizuku/Sui integration can listen for Android clipboard changes through a
				privileged helper. If Shizuku is missing, stopped, or permission is denied, Copywraith
				falls back to normal foreground capture and share-sheet import.
			</div>
			<div class="shizuku-status" class:listening={shizukuState === 'listening'}>
				{shizukuMessage}
			</div>
			<div class="settings-actions inline-actions">
				<Button onclick={handleToggleShizukuClipboard} disabled={shizukuBusy}>
					{shizukuClipboardEnabled ? 'Disable Shizuku Listener' : 'Enable Shizuku Listener'}
				</Button>
				<Button onclick={refreshShizukuStatus}>Check Status</Button>
			</div>

			<div class="section-divider"></div>
			<div class="section-label">Mobile Sync Repair</div>
			<div class="field-hint">
				If the Android list differs from the web UI, reset the pull cursor to re-scan server
				entries without deleting local data.
			</div>
			<Button onclick={handleResetSyncCursor}>Reset Sync Cursor</Button>
		{/if}

		{#if !$isMobile}
			<div class="section-divider"></div>
			<div class="section-label">Keyboard Shortcuts</div>

			<div class="s7-form-group">
				<label for="shortcut-toggle">Toggle Popup</label>
				<input
					id="shortcut-toggle"
					type="text"
					class="s7-input"
					placeholder="CmdOrCtrl+Shift+V"
					bind:value={shortcutTogglePopup}
				/>
			</div>

			<div class="s7-form-group">
				<label for="shortcut-starred">Starred Popup</label>
				<input
					id="shortcut-starred"
					type="text"
					class="s7-input"
					placeholder="CmdOrCtrl+Shift+B"
					bind:value={shortcutStarredPopup}
				/>
			</div>

			<div class="s7-form-group">
				<label for="shortcut-plaintext">Paste as Plaintext</label>
				<input
					id="shortcut-plaintext"
					type="text"
					class="s7-input"
					placeholder="CmdOrCtrl+Shift+Alt+V"
					bind:value={shortcutPastePlaintext}
				/>
			</div>

			<div class="shortcut-hint">
				Use format: CmdOrCtrl+Shift+Key. Leave empty to disable.
			</div>
		{/if}

		<div class="settings-actions s7-actions">
			<Button onclick={onclose}>Cancel</Button>
			<Button variant="primary" onclick={handleSave}>Save</Button>
		</div>
	</div>
{/snippet}

<MovableDialog title="Settings" {onclose} width="380px">
	{@render settingsForm()}
</MovableDialog>

<style>
	.settings-form {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 8px 0;
	}

	label {
		font-size: 12px;
		font-weight: bold;
		margin-bottom: 2px;
	}

	.section-divider {
		border-top: 1px solid #ccc;
		margin: 4px 0 0 0;
	}

	.section-label {
		font-size: 12px;
		font-weight: bold;
		color: #333;
	}

	.shortcut-hint {
		font-size: 10px;
		color: #888;
		line-height: 1.3;
	}

	.field-hint {
		font-size: 10px;
		color: #666;
		line-height: 1.35;
		margin-top: 2px;
	}

	:global(.s7-dialog .s7-form-group) {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	:global(.s7-dialog .s7-input) {
		width: 100%;
		box-sizing: border-box;
	}

	:global(.s7-dialog .settings-actions) {
		display: flex;
		justify-content: flex-end;
		gap: 10px;
		padding-top: 4px;
	}

	:global(.s7-dialog .inline-actions) {
		justify-content: flex-start;
		flex-wrap: wrap;
	}

	.shizuku-status {
		padding: 6px;
		border: 1px solid #999;
		background: #f5f5f5;
		font-size: 10px;
		line-height: 1.35;
		color: #444;
	}

	.shizuku-status.listening {
		border-color: #2f6d35;
		background: #e7f4e7;
	}
</style>
