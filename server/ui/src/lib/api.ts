import type { EntryResponse, ListEntriesResponse, HealthResponse, AuthStatusResponse } from './types';

let API_BASE = '/api';
const SESSION_KEY = 'copywraith_password';

function normalizeBase(base: string): string {
	if (!base) return '/api';
	return base.endsWith('/') ? base.slice(0, -1) : base;
}

function apiUrl(path: string): string {
	const normalizedPath = path.startsWith('/') ? path : `/${path}`;
	return `${API_BASE}${normalizedPath}`;
}

function resolveBlobUrl(blobUrl: string): string {
	if (blobUrl.startsWith('http://') || blobUrl.startsWith('https://')) {
		return blobUrl;
	}

	if (blobUrl.startsWith('/api/')) {
		const apiSuffix = blobUrl.slice('/api'.length);
		return `${normalizeBase(API_BASE)}${apiSuffix}`;
	}

	if (blobUrl.startsWith('/')) {
		return blobUrl;
	}

	return apiUrl(blobUrl);
}

function candidateApiBases(): string[] {
	const candidates = new Set<string>();

	// Default root API path for direct host:port access.
	candidates.add('/api');

	// Prefix-aware API path for reverse proxies serving Copywraith under a subpath.
	const path = typeof window !== 'undefined' ? window.location.pathname : '/';
	const trimmedPath = path.replace(/\/+$/, '');
	if (trimmedPath && trimmedPath !== '/') {
		const slashIdx = trimmedPath.lastIndexOf('/');
		const withoutFile = trimmedPath.endsWith('.html') && slashIdx >= 0
			? trimmedPath.slice(0, slashIdx)
			: trimmedPath;
		if (withoutFile && withoutFile !== '/') {
			candidates.add(`${withoutFile}/api`);
		}
	}

	return Array.from(candidates).map(normalizeBase);
}

// ---------------------------------------------------------------------------
// Session management
// ---------------------------------------------------------------------------

export function getSessionPassword(): string | null {
	return sessionStorage.getItem(SESSION_KEY);
}

export function setSessionPassword(password: string): void {
	sessionStorage.setItem(SESSION_KEY, password);
}

export function clearSession(): void {
	sessionStorage.removeItem(SESSION_KEY);
}

// ---------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------

function buildHeaders(extra: Record<string, string> = {}): Record<string, string> {
	const h: Record<string, string> = { ...extra };
	const password = getSessionPassword();
	if (password) {
		h['Authorization'] = `Bearer ${password}`;
	}
	return h;
}

// ---------------------------------------------------------------------------
// Auth API
// ---------------------------------------------------------------------------

export async function fetchAuthStatus(): Promise<AuthStatusResponse> {
	let lastError: Error | null = null;
	for (const base of candidateApiBases()) {
		try {
			const resp = await fetch(`${base}/auth/status`, { cache: 'no-store' });
			if (!resp.ok) {
				lastError = new Error(`HTTP ${resp.status} from ${base}/auth/status`);
				continue;
			}
			API_BASE = normalizeBase(base);
			return resp.json();
		} catch (e: any) {
			const reason = e?.message ? `: ${e.message}` : '';
			lastError = new Error(`Request failed for ${base}/auth/status${reason}`);
		}
	}

	throw lastError || new Error('Could not reach auth status endpoint');
}

export async function setupPassword(password: string): Promise<void> {
	const resp = await fetch(apiUrl('/auth/setup'), {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ password })
	});
	if (!resp.ok) {
		const data = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` }));
		throw new Error(data.error || `HTTP ${resp.status}`);
	}
	setSessionPassword(password);
}

export async function unlockServer(password: string): Promise<void> {
	const resp = await fetch(apiUrl('/auth/unlock'), {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ password })
	});
	if (!resp.ok) {
		if (resp.status === 401) throw new Error('Incorrect password');
		const data = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` }));
		throw new Error(data.error || `HTTP ${resp.status}`);
	}
	setSessionPassword(password);
}

export async function lockServer(): Promise<void> {
	const resp = await fetch(apiUrl('/auth/lock'), {
		method: 'POST',
		headers: buildHeaders()
	});
	clearSession();
	if (!resp.ok && resp.status !== 401) throw new Error(`HTTP ${resp.status}`);
}

export async function changePassword(oldPassword: string, newPassword: string): Promise<void> {
	const resp = await fetch(apiUrl('/auth/change-password'), {
		method: 'POST',
		headers: buildHeaders({ 'Content-Type': 'application/json' }),
		body: JSON.stringify({ old_password: oldPassword, new_password: newPassword })
	});
	if (!resp.ok) {
		const data = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` }));
		throw new Error(data.error || `HTTP ${resp.status}`);
	}
	setSessionPassword(newPassword);
}

// ---------------------------------------------------------------------------
// Data API
// ---------------------------------------------------------------------------

export async function fetchEntries(params: {
	limit?: number;
	offset?: number;
	search?: string;
	content_type?: string;
	starred_only?: boolean;
}): Promise<ListEntriesResponse> {
	const sp = new URLSearchParams();
	sp.set('limit', String(params.limit ?? 50));
	sp.set('offset', String(params.offset ?? 0));
	if (params.search) sp.set('search', params.search);
	if (params.content_type) sp.set('content_type', params.content_type);
	if (params.starred_only) sp.set('starred_only', 'true');

	const resp = await fetch(`${apiUrl('/entries')}?${sp}`, { headers: buildHeaders() });
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function fetchEntry(id: string): Promise<EntryResponse> {
	const resp = await fetch(apiUrl(`/entries/${id}`), { headers: buildHeaders() });
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function toggleStar(id: string, starred: boolean): Promise<void> {
	const resp = await fetch(apiUrl(`/entries/${id}`), {
		method: 'PATCH',
		headers: buildHeaders({ 'Content-Type': 'application/json' }),
		body: JSON.stringify({ starred })
	});
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
}

export async function deleteEntry(id: string): Promise<void> {
	const resp = await fetch(apiUrl(`/entries/${id}`), {
		method: 'DELETE',
		headers: buildHeaders()
	});
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok && resp.status !== 204) throw new Error(`HTTP ${resp.status}`);
}

export async function fetchHealth(): Promise<HealthResponse> {
	const resp = await fetch(apiUrl('/health'), { headers: buildHeaders() });
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function fetchBlob(blobUrl: string): Promise<Blob> {
	const resp = await fetch(resolveBlobUrl(blobUrl), { headers: buildHeaders() });
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.blob();
}

export async function fetchBlobObjectUrl(blobUrl: string): Promise<string> {
	const blob = await fetchBlob(blobUrl);
	return URL.createObjectURL(blob);
}
