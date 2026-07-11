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
	import type { EntryResponse, AuthStatusResponse } from './lib/types';
	import EntryRow from './lib/EntryRow.svelte';
	import EntryDetail from './lib/EntryDetail.svelte';

	const PAGE_SIZE = 50;

	// Auth state
	type AppScreen = 'loading' | 'setup' | 'unlock' | 'main';
	let screen: AppScreen = $state('loading');
	let authError = $state('');

	// Password form state
	let passwordInput = $state('');
	let passwordConfirm = $state('');
	let isAuthLoading = $state(false);

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

	// Stats
	let statsVersion = $state('v--');

	// Modals
	let detailEntry: EntryResponse | null = $state(null);
	let pendingDeleteId: string | null = $state(null);

	let debounceTimer: ReturnType<typeof setTimeout> | null = null;
	let loadRequestId = 0;

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
		{ key: 'type', label: 'Type', width: '56px' },
		{ key: 'content', label: 'Content' },
		{ key: 'created', label: 'Created', width: '190px' },
		{ key: 'actions', label: 'Actions', width: '132px' }
	];

	onMount(() => {
		void checkAuthStatus();
	});

	async function checkAuthStatus() {
		screen = 'loading';
		try {
			const status: AuthStatusResponse = await api.fetchAuthStatus();
			if (!status.initialized) {
				screen = 'setup';
			} else if (!status.unlocked && !api.getSessionPassword()) {
				screen = 'unlock';
			} else if (api.getSessionPassword()) {
				// Have a session password -- try to use it (will auto-unlock on first request)
				screen = 'main';
				await loadEntries();
			} else {
				screen = 'unlock';
			}
		} catch (e: any) {
			// Server unreachable or unexpected error
			const details = e?.message ? ` ${e.message}` : '';
			authError = `Could not connect to server.${details}`;
			screen = 'unlock';
		}
	}

	async function handleSetup() {
		authError = '';
		if (passwordInput.length < 8) {
			authError = 'Password must be at least 8 characters.';
			return;
		}
		if (passwordInput !== passwordConfirm) {
			authError = 'Passwords do not match.';
			return;
		}
		isAuthLoading = true;
		try {
			await api.setupPassword(passwordInput);
			passwordInput = '';
			passwordConfirm = '';
			screen = 'main';
			await loadEntries();
		} catch (e: any) {
			authError = e.message || 'Setup failed';
		} finally {
			isAuthLoading = false;
		}
	}

	async function handleUnlock() {
		authError = '';
		if (!passwordInput) {
			authError = 'Please enter your password.';
			return;
		}
		isAuthLoading = true;
		try {
			await api.unlockServer(passwordInput);
			passwordInput = '';
			screen = 'main';
			await loadEntries();
		} catch (e: any) {
			authError = e.message || 'Unlock failed';
		} finally {
			isAuthLoading = false;
		}
	}

	async function handleLock() {
		loadRequestId += 1;
		try {
			await api.lockServer();
		} catch (_) {}
		screen = 'unlock';
		entries = [];
		totalEntries = 0;
	}

	async function loadEntries() {
		const requestId = ++loadRequestId;
		const request = {
			offset: currentOffset,
			search: searchQuery || undefined,
			contentType: typeFilter || undefined,
			starredOnly: starredOnly || undefined
		};
		isLoading = true;
		errorMessage = '';
		try {
			const data = await api.fetchEntries({
				limit: PAGE_SIZE,
				offset: request.offset,
				search: request.search,
				content_type: request.contentType,
				starred_only: request.starredOnly
			});
			if (requestId !== loadRequestId) return;

			if (data.entries.length === 0 && data.total > 0 && request.offset >= data.total) {
				currentOffset = Math.floor((data.total - 1) / PAGE_SIZE) * PAGE_SIZE;
				void loadEntries();
				return;
			}

			entries = data.entries;
			totalEntries = data.total;
			hasMore = data.has_more;
			updateStats();
		} catch (e: any) {
			if (requestId !== loadRequestId) return;
			if (e.message === 'Unauthorized') {
				screen = 'unlock';
				return;
			}
			errorMessage = `Failed to load entries: ${e.message}`;
		} finally {
			if (requestId === loadRequestId) {
				isLoading = false;
			}
		}
	}

	async function updateStats() {
		try {
			const health = await api.fetchHealth();
			statsVersion = `v${health.version}`;
		} catch (_) {}
	}

	function debouncedSearch() {
		if (debounceTimer) clearTimeout(debounceTimer);
		loadRequestId += 1;
		isLoading = true;
		debounceTimer = setTimeout(() => {
			currentOffset = 0;
			loadEntries();
		}, 300);
	}

	function handleFilterChange() {
		if (debounceTimer) clearTimeout(debounceTimer);
		currentOffset = 0;
		void loadEntries();
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

	async function handleDownloadById(id: string) {
		try {
			const entry = await api.fetchEntry(id);
			await triggerDownload(entry);
		} catch (e: any) {
			if (e.message === 'Unauthorized') {
				screen = 'unlock';
				return;
			}
			errorMessage = `Failed to download entry: ${e.message}`;
		}
	}

	async function handleDownloadEntry(entry: EntryResponse) {
		try {
			await triggerDownload(entry);
		} catch (e: any) {
			if (e.message === 'Unauthorized') {
				screen = 'unlock';
				return;
			}
			errorMessage = `Failed to download entry: ${e.message}`;
		}
	}

	function extensionFromMime(mimeType: string): string {
		switch (mimeType.toLowerCase()) {
			case 'image/png':
				return 'png';
			case 'image/jpeg':
				return 'jpg';
			case 'image/gif':
				return 'gif';
			case 'image/webp':
				return 'webp';
			case 'image/bmp':
				return 'bmp';
			default:
				return 'png';
		}
	}

	async function triggerDownload(entry: EntryResponse) {
		if (entry.content_type === 'image' && entry.blob_url) {
			const blob = await api.fetchBlob(entry.blob_url);
			const objectUrl = URL.createObjectURL(blob);
			const extension = extensionFromMime(blob.type || 'image/png');
			const a = document.createElement('a');
			a.href = objectUrl;
			a.download = `clipboard-${entry.id.substring(0, 8)}.${extension}`;
			document.body.appendChild(a);
			a.click();
			document.body.removeChild(a);
			URL.revokeObjectURL(objectUrl);
		} else {
			const html = entry.flavors?.text_html ?? (entry.content_type === 'html' ? entry.text_content : null);
			const rtf = entry.flavors?.text_rtf ?? (entry.content_type === 'rtf' ? entry.text_content : null);
			const plain = entry.flavors?.text_plain ?? entry.text_content;

			const payload = html || rtf || plain;
			if (!payload) return;

			const ext = html ? 'html' : rtf ? 'rtf' : 'txt';
			const mime = html ? 'text/html' : rtf ? 'application/rtf' : 'text/plain';
			const blob = new Blob([payload], { type: mime });
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
			if (entries.length === 1 && currentOffset > 0) {
				currentOffset = Math.max(0, currentOffset - PAGE_SIZE);
			}
			void loadEntries();
		} catch (e: any) {
			pendingDeleteId = null;
			errorMessage = `Failed to delete entry: ${e.message}`;
		}
	}

	function prevPage() {
		if (isLoading) return;
		currentOffset = Math.max(0, currentOffset - PAGE_SIZE);
		void loadEntries();
	}

	function nextPage() {
		if (!isLoading && hasMore) {
			currentOffset += PAGE_SIZE;
			void loadEntries();
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

	function handleAuthKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			if (screen === 'setup') handleSetup();
			else if (screen === 'unlock') handleUnlock();
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if screen === 'loading'}
	<div class="window s7-root auth-window">
		<TitleBar title="Copywraith" />
		<div class="auth-content">
			<p class="auth-loading">Loading...</p>
		</div>
	</div>
{:else if screen === 'setup'}
	<div class="window s7-root auth-window">
		<TitleBar title="Copywraith - Create Password" />
		<div class="auth-content">
			<h2 class="auth-heading">Create Password</h2>
			<p class="auth-description">
				Set a password to protect your clipboard data. All entries will be encrypted at rest.
			</p>
			{#if authError}
				<ErrorBanner message={authError} onclose={() => (authError = '')} />
			{/if}
			<!-- svelte-ignore a11y_autofocus -->
			<input
				class="s7-input auth-input"
				type="password"
				placeholder="Password (min. 8 characters)"
				bind:value={passwordInput}
				onkeydown={handleAuthKeydown}
				autofocus
			/>
			<input
				class="s7-input auth-input"
				type="password"
				placeholder="Confirm password"
				bind:value={passwordConfirm}
				onkeydown={handleAuthKeydown}
			/>
			<div class="auth-actions">
				<Button onclick={handleSetup} disabled={isAuthLoading}>
					{isAuthLoading ? 'Setting up...' : 'Create Password'}
				</Button>
			</div>
		</div>
	</div>
{:else if screen === 'unlock'}
	<div class="window s7-root auth-window">
		<TitleBar title="Copywraith - Unlock" />
		<div class="auth-content">
			<h2 class="auth-heading">Unlock Server</h2>
			<p class="auth-description">
				Enter your password to access clipboard data.
			</p>
			{#if authError}
				<ErrorBanner message={authError} onclose={() => (authError = '')} />
			{/if}
			<!-- svelte-ignore a11y_autofocus -->
			<input
				class="s7-input auth-input"
				type="password"
				placeholder="Password"
				bind:value={passwordInput}
				onkeydown={handleAuthKeydown}
				autofocus
			/>
			<div class="auth-actions">
				<Button onclick={handleUnlock} disabled={isAuthLoading}>
					{isAuthLoading ? 'Unlocking...' : 'Unlock'}
				</Button>
			</div>
		</div>
	</div>
{:else}
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
				<Checkbox
					label="Starred only"
					bind:checked={starredOnly}
					onchange={handleFilterChange}
				/>
				<Button onclick={loadEntries}>Refresh</Button>
				{#if api.getSessionPassword()}
					<Button onclick={handleLock}>Lock</Button>
				{/if}
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

			<div class="footer-bar">
				<div class="pagination">
					<Button onclick={prevPage} disabled={isLoading || currentOffset === 0 || totalEntries === 0}
						>&lt; Prev</Button
					>
					<span class="page-info">
						{#if totalEntries === 0}
							0 entries
						{:else}
							{currentOffset + 1}-{Math.min(currentOffset + PAGE_SIZE, totalEntries)} of {totalEntries}
						{/if}
					</span>
					<Button onclick={nextPage} disabled={isLoading || !hasMore || totalEntries === 0}>Next &gt;</Button>
				</div>
				<span class="footer-version">{statsVersion}</span>
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
{/if}

<style>
	/* Auth screens */
	.auth-window {
		max-width: 440px;
		margin: 80px auto 0;
		border: 2px solid #000;
		box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.3), 0 10px 200px rgba(0, 0, 0, 0.6);
		background: #fff;
	}

	.auth-content {
		padding: 24px 28px 28px;
	}

	.auth-heading {
		margin: 0 0 8px;
		font-size: 22px;
		font-weight: bold;
	}

	.auth-description {
		margin: 0 0 18px;
		font-size: 16px;
		color: #444;
		line-height: 1.4;
	}

	.auth-input {
		display: block;
		width: 100%;
		margin-bottom: 12px;
		box-sizing: border-box;
	}

	.auth-actions {
		display: flex;
		justify-content: flex-end;
		margin-top: 8px;
	}

	.auth-loading {
		text-align: center;
		padding: 24px;
		font-size: 18px;
		color: #666;
	}

	/* Main app */
	.window {
		max-width: 1180px;
		margin: 16px auto 0;
		border: 2px solid #000;
		box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.3), 0 10px 200px rgba(0, 0, 0, 0.6);
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

	.table-container {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: auto;
		border-top: 1px solid #000;
	}

	.table-container :global(th) {
		font-size: 18px !important;
		line-height: 1.4 !important;
	}

	.table-container :global(td) {
		font-size: 22px !important;
		line-height: 1.4 !important;
	}

	.footer-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 10px 12px;
		border-top: 1px solid #000;
	}

	.pagination {
		display: flex;
		gap: 8px;
		align-items: center;
		justify-content: flex-start;
		padding: 0;
	}

	.page-info {
		font-size: 18px;
		line-height: 1;
		white-space: nowrap;
	}

	.footer-version {
		font-size: 14px;
		color: #888;
		white-space: nowrap;
	}
</style>
