package ch.lkmc.copywraith.share;

import ch.lkmc.copywraith.share.IShizukuClipboardCallback;

interface IShizukuClipboardService {
    void destroy() = 16777114;
    void start(IShizukuClipboardCallback callback, String callingPackage, String primaryServerUrl, String fallbackServerUrl, String apiKey) = 1;
    void stop() = 2;
    String readCurrentText(String callingPackage) = 3;
    String status() = 4;
}
