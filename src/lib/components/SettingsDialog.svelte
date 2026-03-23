<script lang="ts">
	import { Button, MovableDialog } from '@lkmc/system7-ui';
	import { TauriService } from '$lib/tauri';
	import type { Settings } from '$lib/types';
	import { notify } from '$lib/util/notifications';
	import { onMount } from 'svelte';

	let { onclose }: { onclose: () => void } = $props();

	let serverUrl = $state('');
	let apiKey = $state('');

	onMount(async () => {
		try {
			const settings = await TauriService.getSettings();
			serverUrl = settings.server_url;
			apiKey = settings.api_key;
		} catch (e) {
			console.error('Failed to load settings:', e);
		}
	});

	async function handleSave() {
		try {
			await TauriService.updateSettings({
				server_url: serverUrl,
				api_key: apiKey
			});
			notify('success', 'Settings saved');
			onclose();
		} catch (e) {
			notify('error', `Failed to save settings: ${e}`);
		}
	}
</script>

<MovableDialog title="Settings" {onclose} width="380px">
	<div class="settings-form">
		<div class="s7-form-group">
			<label for="server-url">Server URL</label>
			<input
				id="server-url"
				type="text"
				class="s7-input"
				placeholder="https://your-server.example.com"
				bind:value={serverUrl}
			/>
		</div>

		<div class="s7-form-group">
			<label for="api-key">API Key</label>
			<input
				id="api-key"
				type="password"
				class="s7-input"
				placeholder="Optional"
				bind:value={apiKey}
			/>
		</div>

		<div class="s7-actions">
			<Button onclick={onclose}>Cancel</Button>
			<Button variant="primary" onclick={handleSave}>Save</Button>
		</div>
	</div>
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
</style>
