<script lang="ts">
	import { MovableDialog } from '@lkmc/system7-ui';
	import { onMount } from 'svelte';
	import { TauriService } from '$lib/tauri';
	import { entries, starredOnly } from '$lib/util/clipboardStore';
	import { isMobile } from '$lib/util/platform';
	import { setSyncEndpointStatus, syncEndpointStatus } from '$lib/util/syncStatusStore';

	let entryCount = $derived($entries.length);
	let starredLabel = $derived($starredOnly ? ' (starred)' : '');
	let showSyncDetails = $state(false);
	let configuredLocalUrl: string | null = $state(null);
	let configuredVpnUrl: string | null = $state(null);
	let settingsError: string | null = $state(null);
	let statusRefreshInFlight = false;

	function formatEndpointHost(url: string | null): string {
		if (!url) return '';

		try {
			return new URL(url).host;
		} catch {
			return url.replace(/^https?:\/\//, '').replace(/\/$/, '');
		}
	}

	function formatRole(role: string | null): string {
		if (role === 'vpn') return 'VPN';
		if (!role) return 'server';
		return role.charAt(0).toUpperCase() + role.slice(1);
	}

	let endpointText = $derived.by(() => {
		const status = $syncEndpointStatus;

		if (status.state === 'checking') return 'Sync: checking...';
		if (status.state === 'disabled') return 'Sync: off';
		if (status.state === 'unreachable') return 'Sync: unreachable';

		const host = formatEndpointHost(status.url);
		const role = formatRole(status.role);
		return host ? `Sync: ${role} (${host})` : `Sync: ${role}`;
	});

	let endpointTooltip = $derived.by(() => {
		const status = $syncEndpointStatus;
		if (status.state === 'online' && status.url) {
			return `${formatRole(status.role)} endpoint: ${status.url}`;
		}

		if (status.state === 'disabled') {
			return 'Sync is disabled (no server URL configured)';
		}

		if (status.state === 'unreachable') {
			return 'Configured servers are unreachable';
		}

		return 'Checking sync endpoint';
	});

	let checkedAtText = $derived.by(() => {
		const checkedAt = $syncEndpointStatus.checked_at;
		if (!checkedAt) return 'Not reported yet';

		const date = new Date(checkedAt);
		if (Number.isNaN(date.getTime())) return checkedAt;
		return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
	});

	let configuredEndpointUrl = $derived(configuredLocalUrl ?? configuredVpnUrl);
	let endpointUrlText = $derived(
		$syncEndpointStatus.url ?? configuredEndpointUrl ?? 'No server URL configured'
	);
	let endpointRoleText = $derived(formatRole($syncEndpointStatus.role));
	let syncMessage = $derived($syncEndpointStatus.message ?? endpointTooltip);
	let localUrlText = $derived(configuredLocalUrl ?? 'Not configured');
	let vpnUrlText = $derived(configuredVpnUrl ?? 'Not configured');

	onMount(() => {
		void loadSyncSettings();
	});

	function toggleSyncDetails(event: MouseEvent) {
		event.stopPropagation();
		showSyncDetails = !showSyncDetails;
		if (showSyncDetails) {
			void loadSyncSettings();
			void refreshSyncStatusFromDetails();
		}
	}

	async function loadSyncSettings() {
		try {
			const settings = await TauriService.getSettings();
			configuredLocalUrl = settings.server_url_primary?.trim() || null;
			configuredVpnUrl = settings.server_url_fallback?.trim() || null;
			settingsError = null;
		} catch (e) {
			settingsError = String(e);
		}
	}

	async function refreshSyncStatusFromDetails() {
		if (statusRefreshInFlight) return;
		statusRefreshInFlight = true;

		try {
			const settings = await TauriService.getSettings();
			const role = settings.server_url_primary ? 'local' : settings.server_url_fallback ? 'vpn' : null;
			const url = settings.server_url_primary || settings.server_url_fallback || null;
			setSyncEndpointStatus({
				state: 'checking',
				role,
				url,
				message: 'Sync Details requested a status refresh.'
			});

			const result = await TauriService.syncNow();
			setSyncEndpointStatus(result.endpoint_status);
		} catch (e) {
			setSyncEndpointStatus({
				state: 'unreachable',
				role: configuredLocalUrl ? 'local' : configuredVpnUrl ? 'vpn' : null,
				url: configuredLocalUrl ?? configuredVpnUrl,
				message: String(e)
			});
		} finally {
			statusRefreshInFlight = false;
		}
	}
</script>

<div class="status-bar">
	<span class="status-text">
		{entryCount} item{entryCount !== 1 ? 's' : ''}{starredLabel}
	</span>
	<div class="sync-status-wrap">
		<button
			type="button"
			class="status-endpoint"
			class:online={$syncEndpointStatus.state === 'online'}
			class:disabled={$syncEndpointStatus.state === 'disabled'}
			class:unreachable={$syncEndpointStatus.state === 'unreachable'}
			class:checking={$syncEndpointStatus.state === 'checking'}
			title={endpointTooltip}
			aria-expanded={showSyncDetails}
			onclick={toggleSyncDetails}
		>
			{endpointText}
		</button>

	</div>
	<span class="status-hint">
		{#if $isMobile}
			Tap to copy
		{:else}
			Click to paste &middot; Opt+Click plaintext &middot; ↑/↓ select &middot; Enter paste
		{/if}
	</span>
</div>

{#if showSyncDetails}
	<MovableDialog title="Sync Details" width="360px" onclose={() => (showSyncDetails = false)}>
		<div class="sync-details-body" role="status">
			<div class="sync-details-row">
				<span>State</span>
				<strong>{endpointText}</strong>
			</div>
			<div class="sync-details-row">
				<span>Endpoint</span>
				<strong>{endpointRoleText}</strong>
			</div>
			<div class="sync-details-row">
				<span>URL</span>
				<strong>{endpointUrlText}</strong>
			</div>
			<div class="sync-details-row">
				<span>Local URL</span>
				<strong>{localUrlText}</strong>
			</div>
			<div class="sync-details-row">
				<span>VPN URL</span>
				<strong>{vpnUrlText}</strong>
			</div>
			<div class="sync-details-row">
				<span>Updated</span>
				<strong>{checkedAtText}</strong>
			</div>
			<p>{syncMessage}</p>
			{#if settingsError}
				<p>Settings read failed: {settingsError}</p>
			{/if}
		</div>
	</MovableDialog>
{/if}

<style>
	.status-bar {
		position: relative;
		display: grid;
		grid-template-columns: auto auto 1fr;
		align-items: center;
		gap: 8px;
		padding: 4px 8px;
		border-top: 1px solid #000;
		background: #eee;
		font-size: 13px;
		user-select: none;
		flex-shrink: 0;
	}

	.status-text {
		font-weight: bold;
	}

	.sync-status-wrap {
		position: relative;
		justify-self: start;
	}

	.status-endpoint {
		padding: 2px 6px;
		border: 1px solid #777;
		background: #f5f5f5;
		color: inherit;
		font: inherit;
		font-size: 13px;
		white-space: nowrap;
		cursor: pointer;
	}

	.status-endpoint.online {
		border-color: #2f6d35;
		background: #e7f4e7;
	}

	.status-endpoint.disabled {
		opacity: 0.8;
	}

	.status-endpoint.unreachable {
		border-color: #b35a00;
		background: #fff3e6;
	}

	.status-endpoint.checking {
		border-style: dotted;
	}

	.sync-details-body {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-size: 12px;
	}

	.sync-details-row {
		display: grid;
		grid-template-columns: 76px minmax(0, 1fr);
		gap: 8px;
	}

	.sync-details-row span {
		color: #555;
	}

	.sync-details-row strong {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.sync-details-body p {
		margin: 8px 0;
		line-height: 1.35;
	}

	.status-hint {
		justify-self: end;
		opacity: 0.6;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}

	@media (max-width: 920px) {
		.status-bar {
			grid-template-columns: auto 1fr;
		}

		.status-hint {
			display: none;
		}

		.sync-status-wrap {
			justify-self: end;
		}
	}
</style>
