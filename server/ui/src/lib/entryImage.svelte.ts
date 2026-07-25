import * as api from './api';

export interface EntryImage {
	/** Object URL for the fetched blob, or null while loading / on failure. */
	readonly objectUrl: string | null;
	/** True once the fetch has definitively failed. */
	readonly failed: boolean;
}

/**
 * Fetch an entry's image blob and expose it as an object URL, revoking it when
 * the entry changes or the component is destroyed.
 *
 * `getBlobUrl` is a getter rather than a plain value so the caller's reactive
 * read happens inside this module.
 *
 * The `$derived` here is load-bearing, not stylistic. Reading `entry.blob_url`
 * directly inside the effect makes the effect re-run whenever the prop object is
 * reassigned — which happens on every list refresh, i.e. after every star toggle
 * — revoking the object URL and re-downloading every image on the page. A
 * derived only notifies when its value actually changes, so an unchanged URL
 * string is a no-op. Keep the derived if this is refactored again.
 */
export function createEntryImage(getBlobUrl: () => string | null): EntryImage {
	const blobUrl = $derived(getBlobUrl());

	let objectUrl = $state<string | null>(null);
	let failed = $state(false);

	$effect(() => {
		const url = blobUrl;
		let disposed = false;
		let createdObjectUrl: string | null = null;

		objectUrl = null;
		failed = false;

		if (!url) {
			return;
		}

		api
			.fetchBlobObjectUrl(url)
			.then((fetched) => {
				if (disposed) {
					// The entry changed while this was in flight; the cleanup
					// below has already run and will not see this URL.
					URL.revokeObjectURL(fetched);
					return;
				}
				createdObjectUrl = fetched;
				objectUrl = fetched;
			})
			.catch(() => {
				if (!disposed) {
					failed = true;
				}
			});

		return () => {
			disposed = true;
			if (createdObjectUrl) {
				URL.revokeObjectURL(createdObjectUrl);
			}
		};
	});

	return {
		get objectUrl() {
			return objectUrl;
		},
		get failed() {
			return failed;
		}
	};
}
