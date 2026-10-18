import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import FilesApp from './lib/files/FilesApp.svelte';
import { FILES_LABEL_PREFIX, windowLabel } from './lib/files/window';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  applyAccent,
  applyTheme,
  loadAccent,
  loadTheme,
  setNativeTheme,
} from './lib/stores/appearance.svelte';

const native = (theme: 'light' | 'dark' | null) => {
  getCurrentWindow()
    .setTheme(theme)
    .catch(() => {});
};
setNativeTheme(native);
applyAccent(loadAccent());
applyTheme(loadTheme(), native);
// Another window changed the theme or accent (Settings lives in the main window).
window.addEventListener('storage', (event) => {
  if (event.key === 'rd.theme') applyTheme(loadTheme(), native);
  if (event.key === 'rd.accent') applyAccent(loadAccent());
});

const target = document.getElementById('app');
if (!target) throw new Error('missing #app');

// File browser windows (label `files-<connection id>`) load the same page and show the Files view.
const label = windowLabel();

// The macOS main window is transparent with the system's sidebar material behind it and the toolbar as
// its title bar (tauri.macos.conf.json); `data-glass` switches the theme to it.
if (!label.startsWith(FILES_LABEL_PREFIX) && navigator.userAgent.includes('Macintosh')) {
  document.documentElement.dataset.glass = 'macos';
}
export default label.startsWith(FILES_LABEL_PREFIX)
  ? mount(FilesApp, { target, props: { browserId: label } })
  : mount(App, { target });
