export interface EntryResponse {
	id: string;
	content_type: string;
	text_content: string | null;
	blob_hash: string | null;
	blob_size: number | null;
	source_app: string | null;
	starred: boolean;
	sensitive: boolean;
	created_at: string;
	updated_at: string;
	blob_url: string | null;
}

export interface ListEntriesResponse {
	entries: EntryResponse[];
	total: number;
	has_more: boolean;
}

export interface HealthResponse {
	status: string;
	version: string;
	entries_count?: number;
}

export interface AuthStatusResponse {
	initialized: boolean;
	unlocked: boolean;
}
