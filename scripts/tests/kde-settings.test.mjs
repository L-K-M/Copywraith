import { readFileSync } from 'node:fs';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';

// Render the actual shortcut markup, without the unrelated dialog services.
const source = readFileSync(new URL('../../src/lib/components/SettingsDialog.svelte', import.meta.url), 'utf8');
const begin = source.indexOf('<div class="section-label">Keyboard Shortcuts</div>');
const end = source.indexOf('\n\t\t{/if}', begin);
const markup = source.slice(begin, end);
const component = `<script>let {shortcutStatus} = $props(); let shortcutTogglePopup = ''; let shortcutStarredPopup = ''; let shortcutPastePlaintext = ''; function refreshShortcutStatus() {}</script>${markup}`;

test('KDE unavailable gives recovery guidance without exposing app accelerators', async () => {
    const directory = await mkdtemp(new URL('./kde-render-', import.meta.url));
    try {
        const output = compile(component, { generate: 'server' });
        await writeFile(`${directory}/settings.mjs`, output.js.code);
        const { default: Settings } = await import(`${directory}/settings.mjs`);
        for (const mechanism of ['kde', 'kde_connecting', 'kde_unavailable', 'gnome']) {
            const { body } = render(Settings, { props: { shortcutStatus: { mechanism, commands: [], message: '' } } });
            assert.equal(body.includes('id="shortcut-toggle"'), mechanism === 'gnome');
            if (mechanism === 'kde_unavailable') {
                assert.doesNotMatch(body, /KDE manages these shortcuts/);
                assert.match(body, /unavailable/i);
                assert.match(body, /commands below/);
                assert.match(body, /shortcut-status-warning/);
            }
        }
    } finally { await rm(directory, { recursive: true, force: true }); }
});
