'use strict';

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
const listen = (event, handler) => tauri.event.listen(event, handler);
const $ = (id) => document.getElementById(id);

const SETTINGS_KEY = 'sweep.settings.v1';
const DAY = 86400;
const BADGES = { review: 'Review', danger: 'Danger' };
const FINDER_ICON =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M9 2.5h4.5V7"/><path d="M13.5 2.5 7.5 8.5"/><path d="M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3"/></svg>';

const state = {
  home: '',
  disk: null,
  result: null,
  items: new Map(),
  selected: new Set(),
  active: null,
  busy: false,
  modalDismissable: true,
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

// ------------------------------------------------------------------- items

const selectable = (item) => item.enabled && item.safety !== 'info';

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

function sizeText(item) {
  if (item.sizeUnknown) return '—';
  return `${item.approx ? '≤ ' : ''}${formatBytes(item.bytes)}`;
}

// ------------------------------------------------------------------ render

function show(view) {
  $('welcome').hidden = view !== 'welcome';
  $('scanning').hidden = view !== 'scanning';
  $('results').hidden = view !== 'results';
  $('footer').hidden = view !== 'results';
}

function renderAll() {
  renderDisk();
  renderNav();
  renderCategory();
  renderFooter();
}

function renderDisk() {
  const disk = state.disk;
  if (!disk || !disk.total) return;
  const freeing = Math.min(uniqueBytes(selectedItems()), disk.used);
  const usedPct = (disk.used / disk.total) * 100;
  const freeingPct = (freeing / disk.total) * 100;
  $('diskUsed').style.width = `${usedPct}%`;
  $('diskSelected').style.left = `${usedPct - freeingPct}%`;
  $('diskSelected').style.width = `${freeingPct}%`;
  $('diskLabel').innerHTML = `<strong>${formatBytes(disk.free)} free</strong> of ${formatBytes(disk.total)}`;
}

function renderNav() {
  $('catList').innerHTML = state.result.categories
    .map((cat) => {
      const chosen = cat.items.filter((i) => state.selected.has(i.id));
      const classes = ['cat', cat.id === state.active ? 'active' : '', cat.items.length ? '' : 'empty'].join(' ');
      return `<button class="${classes}" data-cat="${cat.id}" type="button">
          <span class="cat-title">${esc(cat.title)}</span>
          <span class="cat-size">${cat.items.length ? formatBytes(uniqueBytes(cat.items)) : '—'}</span>
          ${chosen.length ? `<span class="cat-sub">${formatBytes(uniqueBytes(chosen))} selected</span>` : ''}
        </button>`;
    })
    .join('');
}

function renderCategory() {
  const cat = state.result.categories.find((c) => c.id === state.active) || state.result.categories[0];
  let html = `<div class="cat-head"><h2>${esc(cat.title)}</h2>`;
  if (cat.items.filter(selectable).length > 1) {
    html += `<div class="bulk">
        <button class="link" data-bulk="all" type="button">All</button>
        <button class="link" data-bulk="none" type="button">None</button>
      </div>`;
  }
  html += '</div>';

  if (!cat.items.length) {
    html += '<div class="empty-state">Nothing here</div>';
  } else if (cat.items.some((i) => i.group)) {
    const groups = new Map();
    for (const item of cat.items) {
      const key = item.group || '';
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(item);
    }
    for (const [group, items] of groups) {
      const last = Math.max(...items.map((i) => i.lastUsed || 0));
      html += `<div class="group-head"><span>${esc(group)}</span><span class="when">${last ? `changed ${ago(last)}` : ''}</span></div>
        <div class="list">${items.map(rowHtml).join('')}</div>`;
    }
  } else {
    html += `<div class="list">${cat.items.map(rowHtml).join('')}</div>`;
  }
  $('catView').innerHTML = html;
}

function rowHtml(item) {
  const canSelect = selectable(item);
  const checkbox =
    item.safety === 'info'
      ? ''
      : `<input type="checkbox" ${state.selected.has(item.id) ? 'checked' : ''} ${canSelect ? '' : 'disabled'} aria-label="Select ${esc(item.title)}">`;
  const badge = BADGES[item.safety] ? `<span class="badge ${item.safety}">${BADGES[item.safety]}</span>` : '';
  return `<div class="row ${canSelect ? '' : 'disabled'}" data-id="${esc(item.id)}">
      <span>${checkbox}</span>
      <div class="row-main" title="${esc(item.paths.map(tilde).join('\n'))}">
        <div class="row-title"><span class="name">${esc(item.title)}</span>${badge}</div>
        ${item.detail ? `<div class="row-detail">${rich(item.detail)}</div>` : ''}
      </div>
      <div class="row-size">${sizeText(item)}</div>
      <button class="icon-btn" data-reveal="${esc(item.id)}" title="Show in Finder" aria-label="Show ${esc(item.title)} in Finder" type="button">${FINDER_ICON}</button>
    </div>`;
}

function renderFooter() {
  const selected = selectedItems();
  const bytes = uniqueBytes(selected);
  $('selSummary').textContent = selected.length ? `${plural(selected.length, 'item')} selected` : '';
  $('cleanBtn').disabled = !selected.length || state.busy;
  $('cleanBtn').textContent = selected.length ? `Clean up ${formatBytes(bytes)}` : 'Clean up';
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

function setBusy(busy) {
  state.busy = busy;
  $('scanBtn').disabled = busy;
  $('welcomeScan').disabled = busy;
  $('foldersBtn').disabled = busy;
  renderFooter();
}

// ------------------------------------------------------------------ modals

function openModal(html, { dismissable = true } = {}) {
  $('modalBox').innerHTML = html;
  $('modal').hidden = false;
  state.modalDismissable = dismissable;
  const focus = $('modalBox').querySelector('.primary:not(:disabled), button:not(:disabled)');
  if (focus) focus.focus();
}

function closeModal() {
  $('modal').hidden = true;
  $('modalBox').innerHTML = '';
}

function showError(title, err) {
  openModal(`<h2>${esc(title)}</h2><p>${esc(String(err))}</p>
    <div class="modal-actions"><button class="primary" data-close type="button">OK</button></div>`);
}

// -------------------------------------------------------------------- scan

async function runScan() {
  if (state.busy) return;
  setBusy(true);
  show('scanning');
  $('scanStep').textContent = 'Scanning';
  const unlisten = await listen('scan-progress', (event) => {
    $('scanStep').textContent = event.payload;
  });
  try {
    const result = await invoke('scan', {
      options: { projectRoots: state.settings.roots, inactiveDays: state.settings.inactiveDays },
    });
    applyScan(result);
  } catch (err) {
    show(state.result ? 'results' : 'welcome');
    showError('Scan failed', err);
  } finally {
    unlisten();
    setBusy(false);
  }
}

function applyScan(result) {
  state.result = result;
  state.home = result.home;
  state.disk = result.disk;
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
  show('results');
  renderAll();
  $('catView').scrollTop = 0;
}

// ------------------------------------------------------------------- clean

function openConfirm() {
  const selected = selectedItems();
  if (!selected.length) return;
  const bytes = uniqueBytes(selected);
  const danger = selected.some((i) => i.safety === 'danger');

  let html = `<h2>Delete ${plural(selected.length, 'item')}?</h2>
    <p>Frees ${selected.some((i) => i.approx) ? 'up to' : 'about'} ${formatBytes(bytes)}. This can't be undone.</p>`;
  if (selected.some((i) => i.category === 'leftovers')) {
    html += '<div class="note review">Includes data from apps that aren\'t installed.</div>';
  }
  if (danger) {
    html += '<label class="note danger"><input type="checkbox" id="ackDanger"> Delete all Docker data, including volumes</label>';
  }
  html += `<div class="modal-actions">
      <button class="ghost" data-close type="button">Cancel</button>
      <button class="primary destructive" id="confirmBtn" type="button" ${danger ? 'disabled' : ''}>Delete</button>
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
  openModal('<h2>Cleaning up</h2><p id="cleanStep"></p><div class="progress"><div id="cleanBar"></div></div>', {
    dismissable: false,
  });
  const unlisten = await listen('clean-progress', (event) => {
    const { done, total, current } = event.payload;
    if ($('cleanBar')) $('cleanBar').style.width = `${total ? (done / total) * 100 : 100}%`;
    if ($('cleanStep')) $('cleanStep').textContent = current;
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
  let html = `<div class="done-hero">
      <div class="big">${report.freed > 0 ? `${formatBytes(report.freed)} freed` : 'Done'}</div>
      <p>${formatBytes(report.diskAfter.free)} free now</p>
    </div>`;
  if (problems.length) html += `<div class="issues">${problems.map(issueHtml).join('')}</div>`;
  html += `<div class="modal-actions">
      <button class="ghost" data-rescan type="button">Scan again</button>
      <button class="primary" data-close type="button">Done</button>
    </div>`;
  openModal(html);
}

function issueHtml(outcome) {
  let hint = '';
  if (outcome.hint) {
    const [text, ...command] = outcome.hint.split('\n');
    hint = `<div class="msg">${esc(text)}</div>`;
    if (command.length) {
      const cmd = command.join('\n');
      hint += `<div class="cmd"><pre>${esc(cmd)}</pre><button class="ghost" data-copy="${esc(cmd)}" type="button">Copy</button></div>`;
    }
    if (text.includes('Full Disk Access')) {
      hint += '<div class="cmd"><button class="ghost" data-fda type="button">Open settings</button></div>';
    }
  }
  return `<div class="issue"><div class="title">${esc(outcome.title)}</div>
    ${outcome.message ? `<div class="msg">${esc(outcome.message)}</div>` : ''}${hint}</div>`;
}

// ----------------------------------------------------------------- folders

async function pickFolders() {
  const options = { directory: true, multiple: true, defaultPath: state.home, title: 'Choose project folders' };
  const picked =
    tauri.dialog && tauri.dialog.open
      ? await tauri.dialog.open(options)
      : await invoke('plugin:dialog|open', { options });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

function openFolders() {
  const draft = { roots: [...state.settings.roots], inactiveDays: state.settings.inactiveDays, note: '' };

  const render = () => {
    const list = draft.roots.length
      ? draft.roots
          .map((root, n) => `<li><span class="path" title="${esc(root)}">${esc(tilde(root))}</span><button class="link" data-remove="${n}" type="button">Remove</button></li>`)
          .join('')
      : '<li><span class="path">~ (home folder)</span></li>';
    openModal(`<h2>Project folders</h2>
      <ul class="roots">${list}</ul>
      <button class="ghost" id="addRoot" type="button">Add folder…</button>
      ${draft.note ? `<p class="small">${esc(draft.note)}</p>` : ''}
      <label class="field">Auto-select projects untouched for
        <input type="number" id="days" min="1" max="3650" value="${draft.inactiveDays}"> days</label>
      <div class="modal-actions">
        <button class="ghost" data-close type="button">Cancel</button>
        <button class="primary" id="saveRoots" type="button">Save</button>
      </div>`);

    const keepDays = () => {
      draft.inactiveDays = clampDays($('days').value);
    };
    $('modalBox').querySelectorAll('[data-remove]').forEach((btn) =>
      btn.addEventListener('click', () => {
        keepDays();
        draft.roots.splice(Number(btn.dataset.remove), 1);
        draft.note = '';
        render();
      }),
    );
    $('addRoot').addEventListener('click', async () => {
      keepDays();
      try {
        const picked = await pickFolders();
        const inside = picked.filter((p) => p === state.home || p.startsWith(state.home + '/'));
        for (const p of inside) if (!draft.roots.includes(p)) draft.roots.push(p);
        draft.note = inside.length < picked.length ? 'Only folders inside your home folder work.' : '';
      } catch (err) {
        draft.note = String(err);
      }
      render();
    });
    $('saveRoots').addEventListener('click', () => {
      keepDays();
      state.settings = { roots: draft.roots, inactiveDays: draft.inactiveDays };
      saveSettings();
      closeModal();
    });
  };
  render();
}

// ------------------------------------------------------------------ events

$('scanBtn').addEventListener('click', runScan);
$('welcomeScan').addEventListener('click', runScan);
$('foldersBtn').addEventListener('click', openFolders);
$('cleanBtn').addEventListener('click', openConfirm);

$('catList').addEventListener('click', (e) => {
  const button = e.target.closest('[data-cat]');
  if (!button) return;
  state.active = button.dataset.cat;
  renderNav();
  renderCategory();
  $('catView').scrollTop = 0;
});

$('catView').addEventListener('click', (e) => {
  const reveal = e.target.closest('[data-reveal]');
  if (reveal) {
    invoke('reveal', { id: reveal.dataset.reveal }).catch((err) => showError("Couldn't show in Finder", err));
    return;
  }
  const bulk = e.target.closest('[data-bulk]');
  if (bulk) {
    const cat = state.result.categories.find((c) => c.id === state.active);
    for (const item of cat.items) {
      // "All" never picks items that delete data for good.
      if (bulk.dataset.bulk === 'none') state.selected.delete(item.id);
      else if (selectable(item) && item.safety !== 'danger') setSelected(item.id, true);
    }
    refreshSelection();
    return;
  }
  const row = e.target.closest('.row');
  if (!row || row.classList.contains('disabled') || e.target.matches('input')) return;
  setSelected(row.dataset.id, !state.selected.has(row.dataset.id));
  refreshSelection();
});

$('catView').addEventListener('change', (e) => {
  if (!e.target.matches('input[type="checkbox"]')) return;
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
      () => (copy.textContent = 'Copy failed'),
    );
    return;
  }
  if (e.target.closest('[data-fda]')) {
    invoke('open_full_disk_access').catch((err) => showError("Couldn't open Settings", err));
    return;
  }
  if (e.target.closest('[data-rescan]')) {
    closeModal();
    runScan();
  }
});

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !$('modal').hidden && state.modalDismissable) closeModal();
});

async function init() {
  try {
    const overview = await invoke('overview');
    state.home = overview.home;
    state.disk = overview.disk;
    renderDisk();
  } catch {
    $('diskLabel').textContent = '';
  }
}

init();
