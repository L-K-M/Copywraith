export type ContentType = 'text' | 'html' | 'rtf' | 'image' | 'file';

export interface ClipboardEntry {
	id: string;
	content_type: ContentType;
	preview: string;
	full_text: string | null;
	has_image: boolean;
	image_base64?: string | null;
	starred: boolean;
	created_at: string;
	updated_at: string;
	source_app: string | null;
}

export interface Settings {
	server_url: string;
	api_key: string;
	shortcut_toggle_popup: string;
	shortcut_starred_popup: string;
	shortcut_paste_plaintext: string;
}
