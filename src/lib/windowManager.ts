import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';

const TITLE_BAR_HEIGHT = 36;

export class WindowManager {
	private appWindow = getCurrentWindow();
	private savedWindowSize: { width: number; height: number } | null = null;
	private isShaded = false;

	async close(): Promise<void> {
		await this.appWindow.hide();
	}

	async startDragging(): Promise<void> {
		await this.appWindow.startDragging();
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
