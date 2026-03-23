import { writable } from 'svelte/store';

interface NotificationItem {
	id: number;
	message: string;
	type: 'success' | 'error' | 'info';
}

let nextId = 1;

export const notifications = writable<NotificationItem[]>([]);

export function notify(type: NotificationItem['type'], message: string, duration = 3000) {
	const id = nextId++;
	notifications.update((n) => [...n, { id, message, type }]);
	setTimeout(() => {
		notifications.update((n) => n.filter((item) => item.id !== id));
	}, duration);
}
