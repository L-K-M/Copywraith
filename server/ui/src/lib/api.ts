import type { EntryResponse, ListEntriesResponse, HealthResponse } from './types';

const API = '/api';

let apiKey = '';

export function setApiKey(key: string) {
	apiKey = key;
}

export function getApiKey(): string {
	return apiKey;
}

function buildHeaders(extra: Record<string, string> = {}): Record<string, string> {
	const h: Record<string, string> = { ...extra };
	if (apiKey) h['Authorization'] = `Bearer ${apiKey}`;
	return h;
}

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
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function fetchEntry(id: string): Promise<EntryResponse> {
	const resp = await fetch(`${API}/entries/${id}`, { headers: buildHeaders() });
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}

export async function toggleStar(id: string, starred: boolean): Promise<void> {
	const resp = await fetch(`${API}/entries/${id}`, {
		method: 'PATCH',
		headers: buildHeaders({ 'Content-Type': 'application/json' }),
		body: JSON.stringify({ starred })
	});
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
}

export async function deleteEntry(id: string): Promise<void> {
	const resp = await fetch(`${API}/entries/${id}`, {
		method: 'DELETE',
		headers: buildHeaders()
	});
	if (!resp.ok && resp.status !== 204) throw new Error(`HTTP ${resp.status}`);
}

export async function fetchHealth(): Promise<HealthResponse> {
	const resp = await fetch(`${API}/health`, { headers: buildHeaders() });
	if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
	return resp.json();
}
