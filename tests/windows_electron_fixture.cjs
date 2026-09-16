const { app, BrowserWindow } = require('electron');
const path = require('node:path');
if (!process.env.UNIMATION_ISOLATED_DESKTOP?.startsWith('UnimationTest-')) {
  throw new Error('Run this fixture through tests/windows_e2e.py');
}
app.setPath('userData', path.resolve(process.argv[2]));
app.commandLine.appendSwitch('force-renderer-accessibility');
app.whenReady().then(async () => {
  app.setAccessibilitySupportEnabled(true);
  const window = new BrowserWindow({
    width: 420, height: 320, x: 500, y: 250, show: false,
    title: 'Unimation Electron fixture',
    webPreferences: { contextIsolation: true, nodeIntegration: false }
  });
  window.removeMenu();
  await window.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(`
    <title>Unimation Electron fixture</title>
    <p id="status">count:0</p>
    <button onclick="document.getElementById('status').textContent='count:'+ ++count">Increment</button>
    <input aria-label="editor"><label><input type="checkbox">Option</label>
    <script>let count=0;</script>`));
  window.showInactive();
});
app.on('window-all-closed', () => app.quit());
