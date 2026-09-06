// Retain in-flight state across polling, but require a fresh token for every UI check.
window.runtimeProbe = window.runtimeProbe || (() => {
    let current;
    return {
        start(token, expected) {
            if (current?.token === token) return current.promise;
            const invoke = window.__TAURI_INTERNALS__?.invoke;
            if (!invoke) return;

            const proof = {token, expected};
            current = proof;
            proof.promise = (async () => {
                try {
                    const platform = await invoke('get_platform');
                    const entries = await invoke('get_entries', {limit: 100, offset: 0, starredOnly: false, search: null});
                    if (platform !== 'android') return;
                    if (expected !== '' && !entries.some(entry => entry.full_text?.includes(expected))) return;
                    // Publish this call's success token only after both IPC replies succeed.
                    proof.completedToken = token;
                } catch { /* Failed IPC cannot publish success. */ }
            })();
            return proof.promise;
        },
        ready(token) {
            if (current?.token !== token || current.completedToken !== token) return false;
            const rendered = document.body?.innerText ?? '';
            return rendered.length > 0 && (current.expected === '' || rendered.includes(current.expected));
        },
    };
})();
