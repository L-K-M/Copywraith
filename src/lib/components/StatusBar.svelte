<script lang="ts">
	import { entries, starredOnly } from '$lib/util/clipboardStore';
	import { syncEndpointStatus } from '$lib/util/syncStatusStore';

	let entryCount = $derived($entries.length);
	let starredLabel = $derived($starredOnly ? ' (starred)' : '');

	function formatEndpointHost(url: string | null): string {
		if (!url) return '';

		try {
			return new URL(url).host;
		} catch {
			return url.replace(/^https?:\/\//, '').replace(/\/$/, '');
		}
	}

	function formatRole(role: string | null): string {
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
</script>

<div class="status-bar">
	<span class="status-text">
		{entryCount} item{entryCount !== 1 ? 's' : ''}{starredLabel}
	</span>
	<span
		class="status-endpoint"
		class:online={$syncEndpointStatus.state === 'online'}
		class:disabled={$syncEndpointStatus.state === 'disabled'}
		class:unreachable={$syncEndpointStatus.state === 'unreachable'}
		title={endpointTooltip}
	>
		{endpointText}
	</span>
	<span class="status-hint">
		Click to paste &middot; Opt+Click plaintext &middot; ↑/↓ select &middot; Enter paste
	</span>
</div>

<style>
	.status-bar {
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

	.status-endpoint {
		padding: 2px 6px;
		border: 1px solid #777;
		background: #f5f5f5;
		font-size: 13px;
		white-space: nowrap;
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

		.status-endpoint {
			justify-self: end;
		}
	}
</style>
