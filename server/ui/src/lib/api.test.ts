import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

type Api = typeof import('./api');

const HTTP = { OK: 200, CREATED: 201, NO_CONTENT: 204, UNAUTHORIZED: 401, FORBIDDEN: 403, ERROR: 500 };
const PASSWORD = 'test password';
const ENTRY_ID = '01JENTRY';

let api: Api;
let session: Map<string, string>;
let fetchMock: ReturnType<typeof vi.fn<typeof fetch>>;

beforeEach(async () => {
	vi.resetModules();
	session = new Map();
	vi.stubGlobal('sessionStorage', {
		getItem: (key: string) => session.get(key) ?? null,
		setItem: (key: string, value: string) => session.set(key, value),
		removeItem: (key: string) => session.delete(key)
	});
	vi.stubGlobal('window', { location: { pathname: '/' } });
	fetchMock = vi.fn<typeof fetch>();
	vi.stubGlobal('fetch', fetchMock);
	api = await import('./api');
	api.setSessionPassword(PASSWORD);
});

afterEach(() => vi.unstubAllGlobals());

const dataRequests = [
	{ name: 'list', run: (api: Api) => api.fetchEntries({}), url: '/api/entries?limit=50&offset=0', method: undefined },
	{ name: 'detail', run: (api: Api) => api.fetchEntry(ENTRY_ID), url: `/api/entries/${ENTRY_ID}`, method: undefined },
	{ name: 'star', run: (api: Api) => api.toggleStar(ENTRY_ID, true), url: `/api/entries/${ENTRY_ID}`, method: 'PATCH' },
	{ name: 'delete', run: (api: Api) => api.deleteEntry(ENTRY_ID), url: `/api/entries/${ENTRY_ID}`, method: 'DELETE' },
	{ name: 'blob', run: (api: Api) => api.fetchBlob(`/api/entries/${ENTRY_ID}/blob`), url: `/api/entries/${ENTRY_ID}/blob`, method: undefined }
];

describe.each(dataRequests)('$name requests', ({ run, url, method }) => {
	it('sends the session password and preserves it on success', async () => {
		fetchMock.mockResolvedValue(new Response('{}', { status: HTTP.OK }));
		await run(api);
		expect(fetchMock).toHaveBeenCalledOnce();
		expect(fetchMock.mock.calls[0][0]).toBe(url);
		expect(fetchMock.mock.calls[0][1]?.method).toBe(method);
		expect(fetchMock.mock.calls[0][1]?.headers).toMatchObject({ Authorization: `Bearer ${PASSWORD}` });
		expect(api.getSessionPassword()).toBe(PASSWORD);
	});

	it('clears the session on unauthorized responses without reading the body', async () => {
		const response = new Response('not JSON', { status: HTTP.UNAUTHORIZED });
		fetchMock.mockResolvedValue(response);
		await expect(run(api)).rejects.toThrow('Unauthorized');
		expect(api.getSessionPassword()).toBeNull();
		expect(response.bodyUsed).toBe(false);
	});

	it.each([HTTP.FORBIDDEN, HTTP.ERROR])('reports HTTP %i without clearing the session or reading the body', async (status) => {
		const response = new Response('not JSON', { status });
		fetchMock.mockResolvedValue(response);
		await expect(run(api)).rejects.toThrow(`HTTP ${status}`);
		expect(api.getSessionPassword()).toBe(PASSWORD);
		expect(response.bodyUsed).toBe(false);
	});

	it('propagates network failure without clearing the session', async () => {
		const failure = new TypeError('Network unavailable');
		fetchMock.mockRejectedValue(failure);
		await expect(run(api)).rejects.toBe(failure);
		expect(api.getSessionPassword()).toBe(PASSWORD);
	});
});

it('encodes filters and retains explicit zero pagination values', async () => {
	const page = { entries: [{ id: ENTRY_ID }], total: 1, has_more: false };
	fetchMock.mockResolvedValue(Response.json(page));
	await expect(api.fetchEntries({ limit: 0, offset: 0, search: 'a & b', content_type: 'html', starred_only: true })).resolves.toEqual(page);
	const url = new URL(String(fetchMock.mock.calls[0][0]), 'http://localhost');
	expect(Object.fromEntries(url.searchParams)).toEqual({ limit: '0', offset: '0', search: 'a & b', content_type: 'html', starred_only: 'true' });
});

it('reads entry JSON and propagates malformed successful JSON', async () => {
	const entry = { id: ENTRY_ID, text_content: 'clipboard text' };
	fetchMock.mockResolvedValueOnce(Response.json(entry)).mockResolvedValueOnce(new Response('not JSON'));
	await expect(api.fetchEntry(ENTRY_ID)).resolves.toEqual(entry);
	await expect(api.fetchEntry(ENTRY_ID)).rejects.toBeInstanceOf(SyntaxError);
	expect(api.getSessionPassword()).toBe(PASSWORD);
});

it('sends the starred body and accepts an empty successful mutation response', async () => {
	fetchMock.mockResolvedValueOnce(new Response(null, { status: HTTP.CREATED })).mockResolvedValueOnce(new Response(null, { status: HTTP.NO_CONTENT }));
	await expect(api.toggleStar(ENTRY_ID, false)).resolves.toBeUndefined();
	expect(fetchMock.mock.calls[0][1]).toEqual({ method: 'PATCH', headers: { Authorization: `Bearer ${PASSWORD}`, 'Content-Type': 'application/json' }, body: '{"starred":false}' });
	await expect(api.deleteEntry(ENTRY_ID)).resolves.toBeUndefined();
});

it('omits authorization when there is no session password', async () => {
	api.clearSession();
	fetchMock.mockResolvedValue(Response.json({}));
	await api.fetchEntries({ search: '', content_type: '', starred_only: false });
	expect(fetchMock).toHaveBeenCalledWith('/api/entries?limit=50&offset=0', { headers: {} });
});

it('preserves blob bytes and MIME type when creating an object URL', async () => {
	const bytes = new Uint8Array([0, 127, 255]);
	fetchMock.mockResolvedValue(new Response(bytes, { headers: { 'Content-Type': 'image/png' } }));
	const createObjectURL = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:test');
	try {
		await expect(api.fetchBlobObjectUrl(`/api/entries/${ENTRY_ID}/blob`)).resolves.toBe('blob:test');
		const blob = createObjectURL.mock.calls[0][0] as Blob;
		expect(blob.type).toBe('image/png');
		expect(new Uint8Array(await blob.arrayBuffer())).toEqual(bytes);
	} finally {
		createObjectURL.mockRestore();
	}
});

it('discovers a reverse-proxy prefix and resolves each supported blob URL form', async () => {
	vi.stubGlobal('window', { location: { pathname: '/clipboard/index.html' } });
	fetchMock.mockResolvedValueOnce(new Response(null, { status: HTTP.ERROR })).mockResolvedValueOnce(Response.json({ initialized: true, unlocked: true }));
	await api.fetchAuthStatus();
	expect(fetchMock.mock.calls.map(([url]) => url)).toEqual(['/api/auth/status', '/clipboard/api/auth/status']);
	fetchMock.mockImplementation(async () => new Response('blob'));
	for (const [input, expected] of [
		['/api/entries/a/blob', '/clipboard/api/entries/a/blob'],
		['entries/a/blob', '/clipboard/api/entries/a/blob'],
		['/other/blob', '/other/blob'],
		['https://example.com/blob', 'https://example.com/blob'],
		['http://example.com/blob', 'http://example.com/blob']
	]) {
		await api.fetchBlob(input);
		expect(fetchMock.mock.lastCall?.[0]).toBe(expected);
	}
});

it('keeps health unauthorized errors distinct from data unauthorized errors', async () => {
	fetchMock.mockResolvedValue(new Response(null, { status: HTTP.UNAUTHORIZED }));
	await expect(api.fetchHealth()).rejects.toThrow('HTTP 401');
	expect(api.getSessionPassword()).toBe(PASSWORD);
});

it('clears the current session when an earlier request returns unauthorized', async () => {
	let respond: (response: Response) => void = () => {};
	fetchMock.mockReturnValue(new Promise((resolve) => { respond = resolve; }));
	const request = api.fetchEntry(ENTRY_ID);
	expect(fetchMock.mock.calls[0][1]?.headers).toEqual({ Authorization: `Bearer ${PASSWORD}` });
	api.setSessionPassword('replacement password');
	respond(new Response(null, { status: HTTP.UNAUTHORIZED }));
	await expect(request).rejects.toThrow('Unauthorized');
	expect(api.getSessionPassword()).toBeNull();
});

it('keeps unlock unauthorized errors distinct and retains the previous session', async () => {
	fetchMock.mockResolvedValue(new Response(null, { status: HTTP.UNAUTHORIZED }));
	await expect(api.unlockServer('wrong password')).rejects.toThrow('Incorrect password');
	expect(api.getSessionPassword()).toBe(PASSWORD);
});

it.each([HTTP.UNAUTHORIZED, HTTP.FORBIDDEN])('clears the session after a lock response, including HTTP %i', async (status) => {
	fetchMock.mockResolvedValue(new Response(null, { status }));
	const result = api.lockServer();
	if (status === HTTP.UNAUTHORIZED) await expect(result).resolves.toBeUndefined();
	else await expect(result).rejects.toThrow(`HTTP ${status}`);
	expect(api.getSessionPassword()).toBeNull();
});
