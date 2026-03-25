import { writable, derived } from 'svelte/store';

/** Current platform: "android", "ios", "macos", "windows", "linux" */
export const platform = writable<string>('');

/** True when running on a mobile platform (Android or iOS) */
export const isMobile = derived(platform, ($p) => $p === 'android' || $p === 'ios');
