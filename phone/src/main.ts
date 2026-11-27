import { mount } from 'svelte';
import '../../src/app.css';
import './phone.css';
import App from './App.svelte';

// Light / dark follows the phone.
const dark = matchMedia('(prefers-color-scheme: dark)');
const theme = () => (document.documentElement.dataset.theme = dark.matches ? 'dark' : 'light');
theme();
dark.addEventListener('change', theme);

mount(App, { target: document.getElementById('app')! });
