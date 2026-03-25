import type { EntryResponse, ListEntriesResponse, HealthResponse, AuthStatusResponse } from './types';

const API = '/api';
const SESSION_KEY = 'copywraith_password';

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
	const resp = await fetch(`${API}/auth/status`, { cache: 'no-store' });
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function setupPassword(password: string): Promise<void> {
	const resp = await fetch(`${API}/auth/setup`, {
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
	const resp = await fetch(`${API}/auth/unlock`, {
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
	const resp = await fetch(`${API}/auth/lock`, {
		method: 'POST',
		headers: buildHeaders()
	});
	clearSession();
	if (!resp.ok && resp.status !== 401) throw new Error(`HTTP ${resp.status}`);
}

export async function changePassword(oldPassword: string, newPassword: string): Promise<void> {
	const resp = await fetch(`${API}/auth/change-password`, {
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

	const resp = await fetch(`${API}/entries?${sp}`, { headers: buildHeaders() });
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function fetchEntry(id: string): Promise<EntryResponse> {
	const resp = await fetch(`${API}/entries/${id}`, { headers: buildHeaders() });
	if (resp.status === 401) {
		clearSession();
		throw new Error('Unauthorized');
	}
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function toggleStar(id: string, starred: boolean): Promise<void> {
	const resp = await fetch(`${API}/entries/${id}`, {
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
	const resp = await fetch(`${API}/entries/${id}`, {
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
	const resp = await fetch(`${API}/health`, { headers: buildHeaders() });
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}
