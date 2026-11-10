// Slate Browser Shell Client Script
(function() {
  'use strict';

  let tabs = [
    { id: 1, title: 'Hacker News', url: 'https://news.ycombinator.com', active: true, suspended: false, icon: '📰' },
    { id: 2, title: 'Wikipedia — Rust', url: 'https://en.wikipedia.org/wiki/Rust_(programming_language)', active: false, suspended: false, icon: '📚' },
    { id: 3, title: 'GitHub — rust-lang/rust', url: 'https://github.com/rust-lang/rust', active: false, suspended: true, icon: '💤' },
    { id: 4, title: 'YouTube — Big Buck Bunny', url: 'https://www.youtube.com/watch?v=jNQXAC9IVRw', active: false, suspended: false, icon: '▶️' }
  ];

  let nextTabId = 5;
  let blockedCount = 142;

  const tabStrip = document.getElementById('tab-strip');
  const omniboxInput = document.getElementById('omnibox-input');
  const globalBlocked = document.getElementById('global-blocked-count');
  const omniboxBlocked = document.getElementById('omnibox-blocked-badge');
  const btnNewTab = document.getElementById('btn-new-tab');

  function renderTabs() {
    tabStrip.innerHTML = '';
    tabs.forEach(tab => {
      const el = document.createElement('div');
      el.className = `tab-item ${tab.active ? 'active' : ''} ${tab.suspended ? 'suspended' : ''}`;
      el.dataset.id = tab.id;

      el.innerHTML = `
        <span class="tab-icon">${tab.icon}</span>
        <span class="tab-title" title="${tab.title}">${tab.title}</span>
        ${tab.suspended ? '<span class="tab-badge">Sleeping</span>' : ''}
        <button class="tab-close" title="Close Tab">×</button>
      `;

      el.addEventListener('click', (e) => {
        if (e.target.classList.contains('tab-close')) {
          e.stopPropagation();
          closeTab(tab.id);
          return;
        }
        activateTab(tab.id);
      });

      tabStrip.appendChild(el);
    });

    globalBlocked.textContent = `${blockedCount} Trackers Blocked`;
    omniboxBlocked.textContent = `🛡️ ${blockedCount} Blocked`;
  }

  function activateTab(id) {
    tabs.forEach(t => {
      t.active = (t.id === id);
      if (t.active && t.suspended) {
        t.suspended = false;
        t.icon = '🌐';
      }
    });
    const active = tabs.find(t => t.active);
    if (active) {
      omniboxInput.value = active.url;
    }
    renderTabs();
  }

  function closeTab(id) {
    if (tabs.length <= 1) return;
    const idx = tabs.findIndex(t => t.id === id);
    const wasActive = tabs[idx].active;
    tabs.splice(idx, 1);
    if (wasActive) {
      const nextActive = tabs[Math.max(0, idx - 1)];
      nextActive.active = true;
      omniboxInput.value = nextActive.url;
    }
    renderTabs();
  }

  function newTab(url = 'https://news.ycombinator.com', title = 'New Tab') {
    tabs.forEach(t => t.active = false);
    const tab = {
      id: nextTabId++,
      title: title,
      url: url,
      active: true,
      suspended: false,
      icon: '🌐'
    };
    tabs.push(tab);
    omniboxInput.value = url;
    renderTabs();
  }

  btnNewTab.addEventListener('click', () => newTab());

  omniboxInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      let val = omniboxInput.value.trim();
      if (!val.startsWith('http://') && !val.startsWith('https://') && !val.startsWith('slate://')) {
        val = 'https://' + val;
      }
      const active = tabs.find(t => t.active);
      if (active) {
        active.url = val;
        active.title = val.replace('https://', '').replace('http://', '').split('/')[0];
        renderTabs();
      }
    }
  });

  document.querySelectorAll('.quick-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.preventDefault();
      const url = btn.dataset.url;
      const title = btn.textContent;
      newTab(url, title);
    });
  });

  renderTabs();
})();
