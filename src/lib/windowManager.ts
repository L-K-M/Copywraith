import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { TauriService } from '$lib/tauri';

type ResizeDirection =
	| 'East'
	| 'North'
	| 'NorthEast'
	| 'NorthWest'
	| 'South'
	| 'SouthEast'
	| 'SouthWest'
	| 'West';

export enum WindowActivitySource {
	Snapshot = 'snapshot',
	Event = 'event'
}

const TITLE_BAR_HEIGHT = 36;
const ACTIVITY_CHANGED = 'window-activity-changed';

export class WindowManager {
	private appWindow = getCurrentWindow();
	private savedWindowSize: { width: number; height: number } | null = null;
	private isShaded = false;

	subscribeActivity(onChange: (active: boolean, source: WindowActivitySource) => void): UnlistenFn {
		let disposed = false;
		let receivedEvent = false;
		let unlisten: UnlistenFn | undefined;

		// Subscribe before reading startup state; newer events beat that snapshot.
		void this.appWindow.listen<boolean>(ACTIVITY_CHANGED, ({ payload }) => {
			if (disposed) return;

			receivedEvent = true;
			onChange(payload, WindowActivitySource.Event);
		}).then(async stop => {
			if (disposed) {
				stop();
				return;
			}

			unlisten = stop;
			const active = await invoke<boolean>('is_window_active');
			if (!disposed && !receivedEvent) onChange(active, WindowActivitySource.Snapshot);
		}).catch(error => {
			console.error('Failed to track window activity:', error);
		});

		return () => {
			disposed = true;
			unlisten?.();
		};
	}

	async close(): Promise<void> {
		try {
			await TauriService.hidePopup();
		} catch {
			await this.appWindow.hide();
		}
	}

	async startDragging(): Promise<void> {
		await this.appWindow.startDragging();
	}

	async startResizeDragging(direction: ResizeDirection): Promise<void> {
		await this.appWindow.startResizeDragging(direction);
	}

	async toggleShade(): Promise<boolean> {
		const factor = await this.appWindow.scaleFactor();
		const physSize = await this.appWindow.innerSize();
		const logicalWidth = physSize.width / factor;
		const logicalHeight = physSize.height / factor;

		if (!this.isShaded) {
			this.savedWindowSize = { width: logicalWidth, height: logicalHeight };
			await this.appWindow.setSize(new LogicalSize(logicalWidth, TITLE_BAR_HEIGHT));
			this.isShaded = true;
		} else {
			const saved = this.savedWindowSize ?? { width: 560, height: 480 };
			await this.appWindow.setSize(new LogicalSize(saved.width, saved.height));
			this.isShaded = false;
		}

		return this.isShaded;
	}
}
