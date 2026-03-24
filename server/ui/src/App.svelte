<script lang="ts">
	import { onMount } from 'svelte';
	import {
		TitleBar,
		DataTable,
		Button,
		Checkbox,
		Dropdown,
		ErrorBanner,
		ConfirmDialog
	} from '@lkmc/system7-ui';
	import * as api from './lib/api';
	import type { EntryResponse } from './lib/types';
	import EntryRow from './lib/EntryRow.svelte';
	import EntryDetail from './lib/EntryDetail.svelte';

	const PAGE_SIZE = 50;
	const API_KEY_STORAGE = 'copywraith_admin_api_key';

	// State
	let entries: EntryResponse[] = $state([]);
	let totalEntries = $state(0);
	let hasMore = $state(false);
	let currentOffset = $state(0);
	let isLoading = $state(true);
	let errorMessage = $state('');

	// Filters
	let searchQuery = $state('');
	let typeFilter = $state('');
	let starredOnly = $state(false);
	let apiKeyValue = $state('');

	// Stats
	let statsTotal = $state('--');
	let statsVersion = $state('--');

	// Modals
	let detailEntry: EntryResponse | null = $state(null);
	let pendingDeleteId: string | null = $state(null);

	let debounceTimer: ReturnType<typeof setTimeout> | null = null;

	const typeOptions = [
		{ value: '', label: 'All types' },
		{ value: 'text', label: 'Text' },
		{ value: 'html', label: 'HTML' },
		{ value: 'rtf', label: 'RTF' },
		{ value: 'image', label: 'Image' },
		{ value: 'file', label: 'File' }
	];

	const columns = [
		{ key: 'star', label: '', width: '30px' },
		{ key: 'type', label: 'Type', width: '92px' },
		{ key: 'content', label: 'Content' },
		{ key: 'created', label: 'Created', width: '190px' },
		{ key: 'actions', label: 'Actions', width: '132px' }
	];

	onMount(() => {
		const saved = localStorage.getItem(API_KEY_STORAGE) || '';
		apiKeyValue = saved;
		api.setApiKey(saved);
		loadEntries();
	});

	async function loadEntries() {
		isLoading = true;
		errorMessage = '';
		try {
			const data = await api.fetchEntries({
				limit: PAGE_SIZE,
				offset: currentOffset,
				search: searchQuery || undefined,
				content_type: typeFilter || undefined,
				starred_only: starredOnly || undefined
			});
			entries = data.entries;
			totalEntries = data.total;
			hasMore = data.has_more;
			updateStats();
		} catch (e: any) {
			errorMessage = `Failed to load entries: ${e.message}`;
		} finally {
			isLoading = false;
		}
	}

	async function updateStats() {
		try {
			const health = await api.fetchHealth();
			statsTotal = `${health.entries_count} entries`;
			statsVersion = `v${health.version}`;
		} catch (_) {}
	}

	function debouncedSearch() {
		if (debounceTimer) clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => {
			currentOffset = 0;
			loadEntries();
		}, 300);
	}

	function handleFilterChange() {
		currentOffset = 0;
		loadEntries();
	}

	function handleApiKeyInput(e: Event) {
		const val = (e.target as HTMLInputElement).value.trim();
		apiKeyValue = val;
		api.setApiKey(val);
		localStorage.setItem(API_KEY_STORAGE, val);
	}

	async function handleToggleStar(id: string, currentStarred: boolean) {
		try {
			await api.toggleStar(id, !currentStarred);
			loadEntries();
		} catch (e: any) {
			errorMessage = `Failed to update entry: ${e.message}`;
		}
	}

	async function handleView(id: string) {
		try {
			const entry = await api.fetchEntry(id);
			detailEntry = entry;
		} catch (e: any) {
			errorMessage = `Failed to load entry: ${e.message}`;
		}
	}

	function handleDownloadById(id: string) {
		api.fetchEntry(id)
			.then((entry) => triggerDownload(entry))
			.catch((e) => {
				errorMessage = `Failed to download entry: ${e.message}`;
			});
	}

	function handleDownloadEntry(entry: EntryResponse) {
		triggerDownload(entry);
	}

	function triggerDownload(entry: EntryResponse) {
		if (entry.content_type === 'image' && entry.blob_url) {
			const a = document.createElement('a');
			a.href = entry.blob_url;
			a.download = `clipboard-${entry.id.substring(0, 8)}.png`;
			document.body.appendChild(a);
			a.click();
			document.body.removeChild(a);
		} else if (entry.text_content) {
			const ext =
				entry.content_type === 'html'
					? 'html'
					: entry.content_type === 'rtf'
						? 'rtf'
						: 'txt';
			const mime =
				entry.content_type === 'html'
					? 'text/html'
					: entry.content_type === 'rtf'
						? 'application/rtf'
						: 'text/plain';
			const blob = new Blob([entry.text_content], { type: mime });
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = `clipboard-${entry.id.substring(0, 8)}.${ext}`;
			document.body.appendChild(a);
			a.click();
			document.body.removeChild(a);
			URL.revokeObjectURL(url);
		}
	}

	function handleAskDelete(id: string) {
		pendingDeleteId = id;
	}

	async function handleConfirmDelete() {
		if (!pendingDeleteId) return;
		try {
			await api.deleteEntry(pendingDeleteId);
			pendingDeleteId = null;
			loadEntries();
		} catch (e: any) {
			pendingDeleteId = null;
			errorMessage = `Failed to delete entry: ${e.message}`;
		}
	}

	function prevPage() {
		currentOffset = Math.max(0, currentOffset - PAGE_SIZE);
		loadEntries();
	}

	function nextPage() {
		if (hasMore) {
			currentOffset += PAGE_SIZE;
			loadEntries();
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			detailEntry = null;
			pendingDeleteId = null;
		}
		if (e.key === '/' && !(e.target instanceof HTMLInputElement)) {
			e.preventDefault();
			document.getElementById('search-input')?.focus();
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="window s7-root">
	<TitleBar title="Copywraith Server Admin" />

	<div class="content">
		{#if errorMessage}
			<ErrorBanner message={errorMessage} onclose={() => (errorMessage = '')} />
		{/if}

		<div class="toolbar">
			<input
				id="search-input"
				class="s7-input search-input"
				type="text"
				placeholder="Search entries..."
				bind:value={searchQuery}
				oninput={debouncedSearch}
			/>
			<Dropdown
				options={typeOptions}
				bind:value={typeFilter}
				onchange={handleFilterChange}
			/>
			<span class="api-key-label">API key</span>
			<input
				class="s7-input api-key-input"
				type="password"
				placeholder="Optional bearer token"
				autocomplete="off"
				value={apiKeyValue}
				oninput={handleApiKeyInput}
			/>
			<Checkbox
				label="Starred only"
				bind:checked={starredOnly}
				onchange={handleFilterChange}
			/>
			<Button onclick={loadEntries}>Refresh</Button>
		</div>

		<div class="table-container">
			<DataTable
				{columns}
				loading={isLoading}
				loadingText="Loading entries..."
				empty={entries.length === 0 && !isLoading}
				emptyText="No clipboard entries found."
			>
				{#each entries as entry (entry.id)}
					<EntryRow
						{entry}
						onstar={handleToggleStar}
						onview={handleView}
						ondownload={handleDownloadById}
						ondelete={handleAskDelete}
					/>
				{/each}
			</DataTable>
		</div>

		{#if totalEntries > PAGE_SIZE}
			<div class="pagination">
				<Button onclick={prevPage} disabled={currentOffset === 0}>&lt; Prev</Button>
				<span class="page-info">
					{currentOffset + 1}-{Math.min(currentOffset + PAGE_SIZE, totalEntries)} of {totalEntries}
				</span>
				<Button onclick={nextPage} disabled={!hasMore}>Next &gt;</Button>
			</div>
		{/if}

		<div class="stats-bar">
			<span>{statsTotal}</span>
			<span>{statsVersion}</span>
		</div>
	</div>
</div>

{#if detailEntry}
	<EntryDetail
		entry={detailEntry}
		onclose={() => (detailEntry = null)}
		ondownload={handleDownloadEntry}
	/>
{/if}

{#if pendingDeleteId}
	<ConfirmDialog
		message="Are you sure you want to delete this entry? This cannot be undone."
		okText="Delete"
		cancelText="Cancel"
		onconfirm={handleConfirmDelete}
		oncancel={() => (pendingDeleteId = null)}
	/>
{/if}

<style>
	.window {
		max-width: 1180px;
		margin: 16px auto;
		border: 2px solid #000;
		box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.3);
		background: #fff;
		display: flex;
		flex-direction: column;
		width: min(1180px, calc(100vw - 32px));
		height: calc(100dvh - 32px);
		max-height: calc(100dvh - 32px);
		box-sizing: border-box;
		overflow: hidden;
	}

	.content {
		padding: 0;
		flex: 1;
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden;
	}

	.toolbar {
		display: flex;
		gap: 8px;
		padding: 12px;
		align-items: center;
		flex-wrap: wrap;
	}

	.search-input {
		flex: 1;
		min-width: 200px;
	}

	.api-key-label {
		font-size: 18px;
		white-space: nowrap;
	}

	.api-key-input {
		flex: 0 0 180px;
		min-width: 140px;
	}

	.table-container {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: auto;
	}

	.table-container :global(th) {
		font-size: 18px !important;
		line-height: 1.4 !important;
	}

	.table-container :global(td) {
		font-size: 22px !important;
		line-height: 1.4 !important;
	}

	.pagination {
		display: flex;
		gap: 8px;
		align-items: center;
		justify-content: center;
		padding: 10px 12px;
		border-top: 1px solid #000;
	}

	.page-info {
		font-size: 14px;
	}

	.stats-bar {
		display: flex;
		gap: 16px;
		padding: 10px 12px;
		font-size: 14px;
		color: #888;
		border-top: 1px solid #ccc;
		margin-top: 0;
	}

	.stats-bar span {
		white-space: nowrap;
	}
</style>
