import { mount } from 'svelte';
import '@lkmc/system7-ui/styles.css';
import './app.css';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) {
	// index.html always provides #app, so this only fires if the served HTML is
	// wrong or truncated. Failing loudly beats mount() throwing on a null target.
	throw new Error('Copywraith admin UI: mount target #app is missing from the page.');
}

mount(App, { target });
