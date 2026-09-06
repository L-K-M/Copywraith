package ch.lkmc.copywraith.share;

import ch.lkmc.copywraith.share.IShizukuClipboardCallback;

interface IShizukuClipboardService {
    void destroy() = 16777114;
    void start(IShizukuClipboardCallback callback) = 1;
    void stop() = 2;
    String readCurrentText() = 3;
    String status() = 4;
}
