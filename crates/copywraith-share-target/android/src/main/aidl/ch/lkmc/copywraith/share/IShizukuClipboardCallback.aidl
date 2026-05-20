package ch.lkmc.copywraith.share;

interface IShizukuClipboardCallback {
    void onClipboardText(String text);
    void onStatus(String state, String message);
}
