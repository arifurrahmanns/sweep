'use strict';

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
const listen = (event, handler) => tauri.event.listen(event, handler);
const $ = (id) => document.getElementById(id);

// Inside the app the window draws a translucent sidebar behind the page.
if (window.__TAURI_INTERNALS__) document.documentElement.classList.add('native');

const SETTINGS_KEY = 'sweep.settings.v1';
const DAY = 86400;
const GAUGE_LENGTH = 2 * Math.PI * 86;
const BADGES = { review: 'Review', danger: 'Danger' };

const ICONS = {
  sliders: '<path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6"/><circle cx="10" cy="4.5" r="1.5"/><circle cx="6" cy="11.5" r="1.5"/>',
  refresh: '<path d="M13.2 8.2a5.2 5.2 0 1 1-1.6-3.9"/><path d="M12.2 1.9v2.9H9.3"/>',
  search: '<circle cx="7" cy="7" r="4.3"/><path d="m10.2 10.2 3.3 3.3"/>',
  lock: '<rect x="3.5" y="7" width="9" height="6.5" rx="1.6"/><path d="M5.5 7V5.2a2.5 2.5 0 0 1 5 0V7"/>',
  done: '<circle cx="8" cy="8" r="6.6"/><path d="m5.2 8.2 2 2 3.7-4.1"/>',
  sparkle: '<path d="M8 2.2 9.3 6.7 13.8 8 9.3 9.3 8 13.8 6.7 9.3 2.2 8 6.7 6.7Z"/>',
  layers: '<path d="M8 2.5 13.5 5.4 8 8.3 2.5 5.4Z"/><path d="m2.5 8.2 5.5 2.9 5.5-2.9"/><path d="m2.5 10.8 5.5 2.9 5.5-2.9"/>',
  code: '<path d="M5.3 4.5 1.8 8l3.5 3.5M10.7 4.5 14.2 8l-3.5 3.5M9.2 2.8 6.8 13.2"/>',
  folder: '<path d="M2 4.6c0-.8.7-1.5 1.5-1.5h2.9l1.5 1.6h4.6c.8 0 1.5.7 1.5 1.5v5.4c0 .8-.7 1.5-1.5 1.5h-9c-.8 0-1.5-.7-1.5-1.5Z"/>',
  box: '<path d="M8 1.8 13.6 4.6v6.8L8 14.2l-5.6-2.8V4.6Z"/><path d="M2.4 4.6 8 7.4l5.6-2.8M8 7.4v6.8"/>',
  download: '<path d="M8 2.2v7.6M4.8 6.6 8 9.8l3.2-3.2M2.6 10.6v1.6c0 .8.7 1.5 1.5 1.5h7.8c.8 0 1.5-.7 1.5-1.5v-1.6"/>',
  containers: '<rect x="1.8" y="7.6" width="3.6" height="3.4" rx=".6"/><rect x="6.2" y="7.6" width="3.6" height="3.4" rx=".6"/><rect x="10.6" y="7.6" width="3.6" height="3.4" rx=".6"/><rect x="6.2" y="3.4" width="3.6" height="3.4" rx=".6"/><path d="M1.8 13.4h12.4"/>',
  eye: '<path d="M1.6 8s2.4-4.4 6.4-4.4S14.4 8 14.4 8s-2.4 4.4-6.4 4.4S1.6 8 1.6 8Z"/><circle cx="8" cy="8" r="2"/>',
  dot: '<circle cx="8" cy="8" r="2.5"/>',
};
const CATEGORY_ICONS = {
  caches: 'layers',
  dev: 'code',
  projects: 'folder',
  leftovers: 'box',
  installers: 'download',
  docker: 'containers',
  manual: 'eye',
};
const icon = (name) => `<svg class="icon" viewBox="0 0 16 16" aria-hidden="true">${ICONS[name]}</svg>`;

const APP_ICON =
  '<svg class="alert-icon" viewBox="100 100 824 824" aria-hidden="true">' +
  '<defs><linearGradient id="alertLogo" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#38d0c4"/><stop offset="1" stop-color="#1563dd"/></linearGradient></defs>' +
  '<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#alertLogo)"/>' +
  '<circle cx="512" cy="512" r="250" fill="none" stroke="#fff" stroke-opacity="0.28" stroke-width="64"/>' +
  '<circle cx="512" cy="512" r="250" fill="none" stroke="#fff" stroke-width="64" stroke-linecap="round" stroke-dasharray="1060 1571" transform="rotate(-90 512 512)"/>' +
  '<path d="M512 392 L543 481 L632 512 L543 543 L512 632 L481 543 L392 512 L481 481 Z" fill="#fff"/></svg>';

const state = {
  home: '',
  disk: null,
  result: null,
  items: new Map(),
  selected: new Set(),
  sections: [],
  active: null,
  busy: false,
  scanning: false,
  step: '',
  modalDismissable: true,
  fullDiskAccess: true,
  settings: loadSettings(),
};

// ---------------------------------------------------------------- settings

function loadSettings() {
  try {
    const saved = JSON.parse(localStorage.getItem(SETTINGS_KEY) || '{}');
    return {
      roots: Array.isArray(saved.roots) ? saved.roots : [],
      inactiveDays: clampDays(saved.inactiveDays),
    };
  } catch {
    return { roots: [], inactiveDays: 30 };
  }
}

function saveSettings() {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(state.settings));
  } catch {
    // Settings just won't persist.
  }
}

function clampDays(value) {
  const days = Math.round(Number(value));
  return Number.isFinite(days) && days >= 1 ? Math.min(days, 3650) : 30;
}

// -------------------------------------------------------------- formatting

/** Decimal units, like Finder. */
function formatBytes(n) {
  if (n < 1000) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = n / 1000;
  let i = 0;
  while (value >= 1000 && i < units.length - 1) {
    value /= 1000;
    i++;
  }
  return `${value.toFixed(i >= 2 && value < 100 ? 1 : 0)} ${units[i]}`;
}

function ago(ts) {
  if (!ts) return '';
  const days = Math.floor((Date.now() / 1000 - ts) / DAY);
  if (days < 1) return 'today';
  if (days < 2) return 'yesterday';
  if (days < 14) return `${days} days ago`;
  if (days < 60) return `${Math.round(days / 7)} weeks ago`;
  if (days < 365) return `${Math.round(days / 30)} months ago`;
  return 'over a year ago';
}

function plural(n, word) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

function esc(text) {
  return String(text).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

/** Escapes text and turns `backticks` into code. */
function rich(text) {
  return esc(text).replace(/`([^`]+)`/g, '<code>$1</code>');
}

function tilde(path) {
  if (!state.home) return path;
  if (path === state.home) return '~';
  return path.startsWith(state.home + '/') ? '~' + path.slice(state.home.length) : path;
}

function listText(names) {
  return names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}` : names[0] || '';
}

// ------------------------------------------------------------------- items

const selectable = (item) => item.enabled && item.safety !== 'info';
/** What "select all" picks: never the options that delete data for good. */
const pickable = (item) => selectable(item) && item.safety !== 'danger';

/** Items that point at the same file (Docker's two options) count once. */
function uniqueBytes(items) {
  const byPath = new Map();
  for (const item of items) byPath.set(item.paths[0] || item.id, item.bytes);
  let total = 0;
  for (const bytes of byPath.values()) total += bytes;
  return total;
}

function selectedItems() {
  return [...state.selected].map((id) => state.items.get(id)).filter(Boolean);
}

function setSelected(id, on) {
  const item = state.items.get(id);
  if (!item || !selectable(item)) return;
  if (!on) {
    state.selected.delete(id);
    return;
  }
  // Options for the same file are alternatives (Docker prune vs. wipe).
  for (const other of state.items.values()) {
    if (other.id !== id && other.paths[0] === item.paths[0]) state.selected.delete(other.id);
  }
  state.selected.add(id);
}

/** Selects every pickable item, or clears them all if they already are. */
function toggleAll(items) {
  const candidates = items.filter(pickable);
  const allOn = candidates.length > 0 && candidates.every((i) => state.selected.has(i.id));
  for (const item of items) {
    if (allOn) state.selected.delete(item.id);
    else if (pickable(item)) setSelected(item.id, true);
  }
  refreshSelection();
}

function sizeText(item) {
  if (item.sizeUnknown) return '—';
  return `${item.approx ? '≤ ' : ''}${formatBytes(item.bytes)}`;
}

function activeCategory() {
  const cats = state.result.categories;
  return cats.find((c) => c.id === state.active) || cats[0];
}

// ------------------------------------------------------------------ render

function show(view) {
  $('start').hidden = view !== 'start';
  $('app').hidden = view !== 'app';
}

function renderAll() {
  renderStart();
  renderDisk();
  if (!state.result) return;
  renderSkipped();
  renderNav();
  renderCategory();
  renderFooter();
}

function renderStart() {
  $('gauge').classList.toggle('spinning', state.scanning);
  $('startScan').hidden = state.scanning;
  $('scanStep').hidden = !state.scanning;
  $('scanStep').textContent = state.step;
  $('accessNote').hidden = state.fullDiskAccess || state.scanning;
}

function renderDisk() {
  const disk = state.disk;
  if (!disk || !disk.total) return;
  $('gaugeUsed').style.strokeDasharray = `${(disk.used / disk.total) * GAUGE_LENGTH} ${GAUGE_LENGTH}`;
  $('gaugeFree').textContent = formatBytes(disk.free);
  $('gaugeOf').textContent = `free of ${formatBytes(disk.total)}`;

  const freeing = Math.min(uniqueBytes(selectedItems()), disk.used);
  const usedPct = (disk.used / disk.total) * 100;
  const freeingPct = (freeing / disk.total) * 100;
  $('diskUsed').style.width = `${usedPct}%`;
  $('diskSelected').style.left = `${usedPct - freeingPct}%`;
  $('diskSelected').style.width = `${freeingPct}%`;
  $('diskLabel').innerHTML = `<strong>${formatBytes(disk.free)}</strong> free of ${formatBytes(disk.total)}`;
}

/** Protected folders the last scan skipped for lack of Full Disk Access. */
function renderSkipped() {
  const skipped = state.result.skipped || [];
  if (!skipped.length) {
    $('skipped').innerHTML = '';
    return;
  }
  const names = esc(listText(skipped));
  $('skipped').innerHTML = state.fullDiskAccess
    ? `<button class="side-note" data-rescan type="button" title="Includes ${names}">${icon('refresh')}<span>Scan again to include ${plural(skipped.length, 'skipped folder')}</span></button>`
    : `<button class="side-note" data-fda type="button" title="Skipped ${names}. Allow Full Disk Access to include them.">${icon('lock')}<span>${plural(skipped.length, 'folder')} skipped</span><strong>Allow</strong></button>`;
}

function renderNav() {
  $('catList').innerHTML = state.result.categories
    .map((cat) => {
      const chosen = cat.items.filter((i) => state.selected.has(i.id));
      const classes = ['cat', cat.id === state.active ? 'active' : '', cat.items.length ? '' : 'empty'].join(' ');
      const size = chosen.length
        ? `<span class="cat-size selected" title="Selected">${formatBytes(uniqueBytes(chosen))}</span>`
        : `<span class="cat-size">${cat.items.length ? formatBytes(uniqueBytes(cat.items)) : ''}</span>`;
      return `<button class="${classes}" data-cat="${esc(cat.id)}" type="button">
          <span class="cat-icon tint-${esc(cat.id)}">${icon(CATEGORY_ICONS[cat.id] || 'dot')}</span>
          <span class="cat-title">${esc(cat.title)}</span>${size}
        </button>`;
    })
    .join('');
}

function renderCategory() {
  const cat = activeCategory();
  $('catTitle').textContent = cat.title;
  state.sections = [];

  if (state.scanning) {
    $('catView').innerHTML = `<div class="placeholder"><div class="spinner"></div><div id="appScanStep">${esc(state.step)}</div></div>`;
    return;
  }
  if (!cat.items.length) {
    $('catView').innerHTML = `<div class="placeholder">${icon('sparkle')}<div>Nothing to clean</div></div>`;
    return;
  }

  if (cat.items.some((i) => i.group)) {
    const groups = new Map();
    for (const item of cat.items) {
      const key = item.group || '';
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(item);
    }
    state.sections = [...groups];
  } else {
    state.sections = [[null, cat.items]];
  }
  $('catView').innerHTML = state.sections.map(([group, items], n) => sectionHtml(group, items, n)).join('');
  $('catView').querySelectorAll('[data-mixed]').forEach((box) => {
    box.indeterminate = true;
  });
}

function sectionHtml(group, items, n) {
  const candidates = items.filter(pickable);
  const on = candidates.every((i) => state.selected.has(i.id));
  const mixed = !on && items.some((i) => state.selected.has(i.id));
  const last = Math.max(0, ...items.map((i) => i.lastUsed || 0));
  const title = group ? esc(group) : plural(items.length, 'item');
  const meta = group && last ? `changed ${ago(last)}` : '';
  const head = `<span class="group-title">${title}</span><span class="group-meta">${meta}</span><span class="group-size">${formatBytes(uniqueBytes(items))}</span>`;

  let header;
  if (candidates.length) {
    const box = `<input class="check" type="checkbox" data-section="${n}" ${on ? 'checked' : ''} ${mixed ? 'data-mixed' : ''} aria-label="Select all">`;
    header = `<label class="group-head">${box}${head}</label>`;
  } else {
    header = `<div class="group-head">${head}</div>`;
  }
  return `<section class="group">${header}<div class="list">${items.map(rowHtml).join('')}</div></section>`;
}

function rowHtml(item) {
  const canSelect = selectable(item);
  const box =
    item.safety === 'info'
      ? '<span class="check-spacer"></span>'
      : `<input class="check" type="checkbox" ${state.selected.has(item.id) ? 'checked' : ''} ${canSelect ? '' : 'disabled'} aria-label="Select ${esc(item.title)}">`;
  const badge = BADGES[item.safety] ? `<span class="badge ${item.safety}">${BADGES[item.safety]}</span>` : '';
  return `<div class="row ${canSelect ? '' : 'disabled'}" data-id="${esc(item.id)}">
      ${box}
      <div class="row-main" title="${esc(item.paths.map(tilde).join('\n'))}">
        <div class="row-title"><span class="name">${esc(item.title)}</span>${badge}</div>
        ${item.detail ? `<div class="row-detail">${rich(item.detail)}</div>` : ''}
      </div>
      <button class="tool reveal" data-reveal="${esc(item.id)}" title="Show in Finder" aria-label="Show ${esc(item.title)} in Finder" type="button">${icon('search')}</button>
      <div class="row-size">${sizeText(item)}</div>
    </div>`;
}

function renderFooter() {
  const selected = selectedItems();
  $('selSummary').innerHTML = selected.length
    ? `<strong>${formatBytes(uniqueBytes(selected))}</strong> selected · ${plural(selected.length, 'item')}`
    : 'Nothing selected';
  $('cleanBtn').disabled = !selected.length || state.busy;
}

/** Re-render after a selection change, keeping the scroll position. */
function refreshSelection() {
  const top = $('catView').scrollTop;
  renderCategory();
  $('catView').scrollTop = top;
  renderNav();
  renderFooter();
  renderDisk();
}

function openCategory(id) {
  state.active = id;
  renderNav();
  renderCategory();
  $('catView').scrollTop = 0;
  updateToolbarEdge();
}

function moveCategory(step) {
  const cats = state.result.categories;
  const index = cats.indexOf(activeCategory());
  const next = cats[Math.max(0, Math.min(cats.length - 1, index + step))];
  if (next && next.id !== state.active) openCategory(next.id);
}

function updateToolbarEdge() {
  $('toolbar').classList.toggle('scrolled', $('catView').scrollTop > 0);
}

function setBusy(busy) {
  state.busy = busy;
  $('startScan').disabled = busy;
  $('rescanBtn').disabled = busy;
  document.querySelectorAll('[data-settings]').forEach((button) => {
    button.disabled = busy;
  });
  renderFooter();
}

// ------------------------------------------------------------------ dialogs

function openModal(html, { dismissable = true, wide = false } = {}) {
  $('modalBox').className = wide ? 'modal wide' : 'modal';
  $('modalBox').innerHTML = html;
  $('modal').hidden = false;
  state.modalDismissable = dismissable;
  if (document.activeElement) document.activeElement.blur();
}

/** Return presses the dialog's default (blue) button, like a Mac alert. */
function pressDefaultButton() {
  const buttons = $('modalBox').querySelectorAll('.modal-actions .primary');
  const button = buttons[buttons.length - 1];
  if (button && !button.disabled) button.click();
}

function closeModal() {
  $('modal').hidden = true;
  $('modalBox').innerHTML = '';
}

/** A centered macOS-style alert header. `text` is already HTML. */
function alertHead(iconHtml, title, text = '') {
  return `<div class="alert-head">${iconHtml}<h2>${esc(title)}</h2>${text ? `<p>${text}</p>` : ''}</div>`;
}

function showError(title, err) {
  openModal(`${alertHead(APP_ICON, title, esc(String(err)))}
    <div class="modal-actions"><button class="btn primary" data-close type="button">OK</button></div>`);
}

// -------------------------------------------------------------------- scan

function setStep(text) {
  state.step = text;
  $('scanStep').textContent = text;
  if ($('appScanStep')) $('appScanStep').textContent = text;
}

async function runScan() {
  if (state.busy) return;
  setBusy(true);
  state.scanning = true;
  state.step = 'Scanning…';
  renderStart();
  if (state.result) {
    $('app').classList.add('scanning');
    renderCategory();
  }
  const unlisten = await listen('scan-progress', (event) => setStep(event.payload));
  try {
    const result = await invoke('scan', {
      options: { projectRoots: state.settings.roots, inactiveDays: state.settings.inactiveDays },
    });
    state.scanning = false;
    applyScan(result);
  } catch (err) {
    state.scanning = false;
    renderStart();
    if (state.result) renderCategory();
    showError('Scan failed', err);
  } finally {
    state.scanning = false;
    $('app').classList.remove('scanning');
    unlisten();
    setBusy(false);
  }
}

function applyScan(result) {
  state.result = result;
  state.home = result.home;
  state.disk = result.disk;
  state.fullDiskAccess = result.fullDiskAccess;
  state.items = new Map();
  state.selected = new Set();
  for (const cat of result.categories) {
    for (const item of cat.items) {
      state.items.set(item.id, item);
      if (item.preselect && selectable(item)) state.selected.add(item.id);
    }
  }
  if (!result.categories.some((c) => c.id === state.active && c.items.length)) {
    state.active = (result.categories.find((c) => c.items.length) || result.categories[0]).id;
  }
  show('app');
  renderAll();
  $('catView').scrollTop = 0;
  updateToolbarEdge();
}

// ------------------------------------------------------------------- clean

function openConfirm() {
  const selected = selectedItems();
  if (!selected.length || state.busy) return;
  const bytes = uniqueBytes(selected);
  const danger = selected.some((i) => i.safety === 'danger');

  let html = alertHead(
    APP_ICON,
    `Delete ${plural(selected.length, 'item')}?`,
    `Frees ${selected.some((i) => i.approx) ? 'up to' : 'about'} ${formatBytes(bytes)}. This can't be undone.`,
  );
  if (selected.some((i) => i.category === 'leftovers')) {
    html += '<div class="note">Includes data from apps that aren\'t installed.</div>';
  }
  if (danger) {
    html += '<label class="note danger"><input class="check" type="checkbox" id="ackDanger"> Delete all Docker data, including volumes</label>';
  }
  html += `<div class="modal-actions">
      <button class="btn" data-close type="button">Cancel</button>
      <button class="btn primary destructive" id="confirmBtn" type="button" ${danger ? 'disabled' : ''}>Delete</button>
    </div>`;
  openModal(html);
  if (danger) {
    $('ackDanger').addEventListener('change', (e) => {
      $('confirmBtn').disabled = !e.target.checked;
    });
  }
  $('confirmBtn').addEventListener('click', () => runClean(selected));
}

async function runClean(items) {
  setBusy(true);
  openModal(
    `${alertHead(APP_ICON, 'Cleaning Up…')}
     <div class="progress"><div id="cleanBar"></div></div>
     <div class="progress-step" id="cleanStep">&nbsp;</div>`,
    { dismissable: false },
  );
  const unlisten = await listen('clean-progress', (event) => {
    const { done, total, current } = event.payload;
    if ($('cleanBar')) $('cleanBar').style.width = `${total ? (done / total) * 100 : 100}%`;
    if ($('cleanStep') && current) $('cleanStep').textContent = current;
  });
  try {
    const report = await invoke('clean', { ids: items.map((i) => i.id) });
    applyReport(report);
    showDone(report);
  } catch (err) {
    closeModal();
    showError('Cleaning stopped', err);
  } finally {
    unlisten();
    setBusy(false);
  }
}

function applyReport(report) {
  const removed = new Set(report.outcomes.filter((o) => o.ok).map((o) => o.id));
  // A file deleted outright takes its alternative options with it.
  const gonePaths = new Set(
    [...removed].map((id) => state.items.get(id)).filter((i) => i && !i.approx).map((i) => i.paths[0]),
  );
  for (const cat of state.result.categories) {
    cat.items = cat.items.filter((i) => !removed.has(i.id) && !gonePaths.has(i.paths[0]));
  }
  state.items = new Map();
  for (const cat of state.result.categories) for (const item of cat.items) state.items.set(item.id, item);
  state.selected = new Set([...state.selected].filter((id) => state.items.has(id)));
  state.disk = report.diskAfter;
  renderAll();
}

function showDone(report) {
  const problems = report.outcomes.filter((o) => !o.ok);
  let html = alertHead(
    `<div class="alert-icon ok">${icon('done')}</div>`,
    report.freed > 0 ? `${formatBytes(report.freed)} freed` : 'Done',
    `${formatBytes(report.diskAfter.free)} free now`,
  );
  if (problems.length) html += `<div class="issues">${problems.map(issueHtml).join('')}</div>`;
  html += `<div class="modal-actions">
      <button class="btn" data-rescan type="button">Scan Again</button>
      <button class="btn primary" data-close type="button">Done</button>
    </div>`;
  openModal(html, { wide: problems.length > 0 });
}

function issueHtml(outcome) {
  let html = `<div class="issue"><div class="issue-title">${esc(outcome.title)}</div>`;
  if (outcome.message) html += `<div class="issue-msg">${esc(outcome.message)}</div>`;
  const actions = [];
  if (outcome.hint) {
    const [text, ...command] = outcome.hint.split('\n');
    html += `<div class="issue-msg">${esc(text)}</div>`;
    if (command.length) {
      const cmd = command.join('\n');
      actions.push(`<pre>${esc(cmd)}</pre><button class="btn small" data-copy="${esc(cmd)}" type="button">Copy</button>`);
    }
    if (text.includes('Full Disk Access')) {
      actions.push('<button class="btn small" data-fda type="button">Open Settings</button>');
    }
  }
  if (outcome.needsAdmin) {
    actions.push(`<button class="btn small primary" data-fix="${esc(outcome.id)}" type="button">Fix with Password</button>`);
  }
  if (actions.length) html += `<div class="issue-actions">${actions.join('')}</div>`;
  return `${html}</div>`;
}

/** Takes back root-owned files (macOS asks for the password) and cleans again. */
async function fixItem(button) {
  const id = button.dataset.fix;
  const title = (state.items.get(id) || {}).title || '';
  button.disabled = true;
  button.textContent = 'Waiting for Password…';
  let html;
  try {
    const report = await invoke('fix_and_clean', { id });
    applyReport(report);
    const outcome = report.outcomes[0];
    html = outcome.ok
      ? `<div class="issue"><div class="issue-title">${esc(outcome.title)}</div><div class="issue-msg ok">Deleted. ${formatBytes(report.diskAfter.free)} free now.</div></div>`
      : issueHtml(outcome);
  } catch (err) {
    html = issueHtml({ id, title, needsAdmin: true, message: String(err) });
  }
  button.closest('.issue').outerHTML = html;
}

// ---------------------------------------------------------------- settings

async function pickFolders() {
  const options = { directory: true, multiple: true, defaultPath: state.home, title: 'Choose project folders' };
  const picked =
    tauri.dialog && tauri.dialog.open
      ? await tauri.dialog.open(options)
      : await invoke('plugin:dialog|open', { options });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/** Settings apply as soon as they change, like a Mac settings window. */
function openSettings() {
  if (state.busy) return;
  let chosen = -1;
  let error = '';

  const render = () => {
    const roots = state.settings.roots;
    const rows = roots.length
      ? roots
          .map((root, n) => `<li class="${n === chosen ? 'selected' : ''}" data-root="${n}" title="${esc(root)}">${esc(tilde(root))}</li>`)
          .join('')
      : '<li class="placeholder-row">Home folder</li>';
    openModal(
      `<h2 class="sheet-title">Settings</h2>
      <div class="field-label">Project folders</div>
      <div class="table">
        <ul class="roots">${rows}</ul>
        <div class="table-bar">
          <button id="addRoot" type="button" title="Add Folder" aria-label="Add folder">+</button>
          <button id="removeRoot" type="button" title="Remove Folder" aria-label="Remove folder" ${chosen < 0 ? 'disabled' : ''}>−</button>
        </div>
      </div>
      ${error ? `<p class="sheet-error">${esc(error)}</p>` : ''}
      <label class="field">Auto-select projects untouched for
        <input type="number" id="days" min="1" max="3650" value="${state.settings.inactiveDays}"> days</label>
      <div class="modal-actions"><button class="btn primary" data-close type="button">Done</button></div>`,
      { wide: true },
    );

    $('modalBox').querySelector('.roots').addEventListener('click', (e) => {
      const row = e.target.closest('[data-root]');
      chosen = row ? Number(row.dataset.root) : -1;
      render();
    });
    $('removeRoot').addEventListener('click', () => {
      if (chosen < 0) return;
      state.settings.roots.splice(chosen, 1);
      saveSettings();
      chosen = -1;
      error = '';
      render();
    });
    $('addRoot').addEventListener('click', async () => {
      try {
        const picked = await pickFolders();
        const inside = picked.filter((p) => p === state.home || p.startsWith(state.home + '/'));
        for (const p of inside) if (!state.settings.roots.includes(p)) state.settings.roots.push(p);
        saveSettings();
        error = inside.length < picked.length ? 'Only folders inside your home folder work.' : '';
      } catch (err) {
        error = String(err);
      }
      chosen = -1;
      render();
    });
    $('days').addEventListener('input', (e) => {
      if (e.target.value === '') return;
      state.settings.inactiveDays = clampDays(e.target.value);
      saveSettings();
    });
    $('days').addEventListener('change', (e) => {
      e.target.value = state.settings.inactiveDays;
    });
  };
  render();
}

// ------------------------------------------------------------------ events

$('startScan').addEventListener('click', runScan);
$('rescanBtn').addEventListener('click', runScan);
$('cleanBtn').addEventListener('click', openConfirm);
$('catView').addEventListener('scroll', updateToolbarEdge, { passive: true });

$('catList').addEventListener('click', (e) => {
  const button = e.target.closest('[data-cat]');
  if (button) openCategory(button.dataset.cat);
});

$('catView').addEventListener('click', (e) => {
  const reveal = e.target.closest('[data-reveal]');
  if (reveal) {
    invoke('reveal', { id: reveal.dataset.reveal }).catch((err) => showError("Couldn't show in Finder", err));
    return;
  }
  const row = e.target.closest('.row');
  if (!row || row.classList.contains('disabled') || e.target.matches('input')) return;
  setSelected(row.dataset.id, !state.selected.has(row.dataset.id));
  refreshSelection();
});

$('catView').addEventListener('change', (e) => {
  if (!e.target.matches('input[type="checkbox"]')) return;
  if (e.target.dataset.section !== undefined) {
    const section = state.sections[Number(e.target.dataset.section)];
    if (section) toggleAll(section[1]);
    return;
  }
  setSelected(e.target.closest('.row').dataset.id, e.target.checked);
  refreshSelection();
});

$('modal').addEventListener('click', (e) => {
  if (e.target.closest('[data-close]') || (e.target === $('modal') && state.modalDismissable)) {
    closeModal();
    return;
  }
  const copy = e.target.closest('[data-copy]');
  if (copy) {
    navigator.clipboard.writeText(copy.dataset.copy).then(
      () => (copy.textContent = 'Copied'),
      () => (copy.textContent = 'Copy Failed'),
    );
    return;
  }
  const fix = e.target.closest('[data-fix]');
  if (fix) fixItem(fix);
});

// Buttons that can appear anywhere in the window.
document.addEventListener('click', (e) => {
  if (e.target.closest('[data-fda]')) {
    invoke('open_full_disk_access').catch((err) => showError("Couldn't open Settings", err));
  } else if (e.target.closest('[data-settings]')) {
    openSettings();
  } else if (e.target.closest('[data-rescan]')) {
    closeModal();
    runScan();
  }
});

// Keyboard: ⌘R scan, ⌘, settings, ⌘A select all, ⌘⌫ clean up, ↑↓ categories.
document.addEventListener('keydown', (e) => {
  const mod = e.metaKey || e.ctrlKey;
  if (!$('modal').hidden) {
    if (e.key === 'Escape' && state.modalDismissable) closeModal();
    if (e.key === 'Enter' && !(e.target instanceof Element && e.target.closest('button'))) {
      e.preventDefault();
      pressDefaultButton();
    }
    return;
  }
  if (mod && e.code === 'KeyR') {
    runScan();
    return;
  }
  if (mod && e.code === 'Comma') {
    e.preventDefault();
    openSettings();
    return;
  }
  const typing = e.target instanceof Element && e.target.matches('input[type="number"]');
  if (!state.result || $('app').hidden || state.scanning || typing) return;
  if (mod && e.code === 'KeyA') {
    e.preventDefault();
    toggleAll(activeCategory().items);
  } else if (mod && e.key === 'Backspace') {
    e.preventDefault();
    openConfirm();
  } else if (!mod && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
    e.preventDefault();
    moveCategory(e.key === 'ArrowDown' ? 1 : -1);
  }
});

// Notice when the user comes back from turning on Full Disk Access.
window.addEventListener('focus', async () => {
  try {
    const { fullDiskAccess } = await invoke('overview');
    if (fullDiskAccess === state.fullDiskAccess) return;
    state.fullDiskAccess = fullDiskAccess;
    renderStart();
    if (state.result) renderSkipped();
  } catch {
    // Keep the last known state.
  }
});

// --------------------------------------------------------- native app feel

// Behave like a Mac app, not a web page: no right-click menu (Reload,
// Inspect Element, Look Up), no dragging bits of the page around, and no
// browser shortcuts for reload, print, find, save, zoom or the inspector.
const BROWSER_SHORTCUTS = new Set(['KeyR', 'KeyP', 'KeyF', 'KeyG', 'KeyS', 'KeyU', 'KeyO', 'Equal', 'Minus', 'Digit0', 'NumpadAdd', 'NumpadSubtract', 'Numpad0']);
const INSPECTOR_SHORTCUTS = new Set(['KeyI', 'KeyJ', 'KeyC']);

document.addEventListener('contextmenu', (e) => e.preventDefault());
document.addEventListener('dragstart', (e) => e.preventDefault());
document.addEventListener(
  'keydown',
  (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (
      ['F5', 'F7', 'F12'].includes(e.key) ||
      (mod && BROWSER_SHORTCUTS.has(e.code)) ||
      (mod && (e.altKey || e.shiftKey) && INSPECTOR_SHORTCUTS.has(e.code))
    ) {
      e.preventDefault();
    }
  },
  true,
);

// -------------------------------------------------------------------- init

async function init() {
  document.querySelectorAll('[data-settings]').forEach((button) => {
    button.innerHTML = icon('sliders');
  });
  $('rescanBtn').innerHTML = icon('refresh');
  try {
    const overview = await invoke('overview');
    state.home = overview.home;
    state.disk = overview.disk;
    state.fullDiskAccess = overview.fullDiskAccess;
  } catch {
    // The disk numbers just stay empty.
  }
  renderStart();
  renderDisk();
}

init();
