import { mount } from 'svelte';
import '@lkmc/system7-ui/styles.css';
import './app.css';
import App from './App.svelte';

mount(App, {
	target: document.getElementById('app')
});
