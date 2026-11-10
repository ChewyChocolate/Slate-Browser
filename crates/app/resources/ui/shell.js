// Slate Browser Shell Client Script
(function() {
  'use strict';

  let nextTabId = 2;

  // Real tabs state
  let tabs = [
    {
      id: 1,
      title: 'New Tab',
      url: 'slate://newtab',
      active: true,
      icon: '🌐',
      blocked: null
    }
  ];

  const tabStrip = document.getElementById('tab-strip');
  const omniboxInput = document.getElementById('omnibox-input');
  const globalBlocked = document.getElementById('global-blocked-count');
  const omniboxBlocked = document.getElementById('omnibox-blocked-badge');
  const btnNewTab = document.getElementById('btn-new-tab');
  const btnBack = document.getElementById('btn-back');
  const btnForward = document.getElementById('btn-forward');
  const btnReload = document.getElementById('btn-reload');
  const webFrame = document.getElementById('active-web-frame');
  const splashCard = document.getElementById('new-tab-splash');

  function getSiteIcon(url) {
    return '🌐';
  }

  // Title is just the hostname until the engine reports the real page title.
  function getSiteTitle(url) {
    if (!url || url.startsWith('slate://')) return 'New Tab';
    try {
      return new URL(url).hostname.replace(/^www\./, '');
    } catch (e) {
      return url;
    }
  }

  function renderTabs() {
    tabStrip.innerHTML = '';
    tabs.forEach(tab => {
      const el = document.createElement('div');
      el.className = `tab-item ${tab.active ? 'active' : ''}`;
      el.dataset.id = tab.id;

      el.innerHTML = `
        <span class="tab-icon">${tab.icon}</span>
        <span class="tab-title" title="${tab.title}">${tab.title}</span>
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

    const active = tabs.find(t => t.active);
    // '-' means "no real data yet" (pipeline not wired to the UI).
    const activeBlocked = active && active.blocked !== null ? active.blocked : '-';
    globalBlocked.textContent = '- Trackers Blocked';
    omniboxBlocked.textContent = `🛡️ ${activeBlocked} Blocked`;
  }

  function renderViewport(tab) {
    if (!tab || tab.url === 'slate://newtab') {
      webFrame.style.display = 'none';
      splashCard.style.display = 'flex';
      omniboxInput.value = '';
      omniboxInput.placeholder = 'Search or enter web address';
    } else {
      splashCard.style.display = 'none';
      webFrame.style.display = 'block';
      if (webFrame.src !== tab.url) {
        webFrame.src = tab.url;
      }
      omniboxInput.value = tab.url;
    }
  }

  function navigateTo(rawUrl) {
    let url = rawUrl.trim();
    if (!url) return;
    if (!url.startsWith('http://') && !url.startsWith('https://') && !url.startsWith('slate://')) {
      url = 'https://' + url;
    }

    const active = tabs.find(t => t.active);
    if (!active) return;

    active.url = url;
    active.title = getSiteTitle(url);
    active.icon = getSiteIcon(url);

    // Blocked counts stay null until the real blocking pipeline reports them over IPC.
    active.blocked = null;

    renderViewport(active);
    renderTabs();
    console.log('SLATE_UI_NAVIGATE:' + url);
  }

  function activateTab(id) {
    tabs.forEach(t => t.active = (t.id === id));
    const active = tabs.find(t => t.active);
    renderViewport(active);
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
      renderViewport(nextActive);
    }
    renderTabs();
  }

  function createNewTab(url = 'slate://newtab') {
    tabs.forEach(t => t.active = false);
    const tab = {
      id: nextTabId++,
      title: getSiteTitle(url),
      url: url,
      active: true,
      icon: getSiteIcon(url),
      blocked: null
    };
    tabs.push(tab);
    renderViewport(tab);
    renderTabs();
    if (url !== 'slate://newtab') {
      navigateTo(url);
    }
  }

  btnNewTab.addEventListener('click', () => createNewTab());

  omniboxInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      navigateTo(omniboxInput.value);
    }
  });

  btnReload.addEventListener('click', () => {
    const active = tabs.find(t => t.active);
    if (active && active.url !== 'slate://newtab') {
      webFrame.src = active.url;
    }
  });

  btnBack.addEventListener('click', () => {
    try { webFrame.contentWindow.history.back(); } catch (e) {}
  });

  btnForward.addEventListener('click', () => {
    try { webFrame.contentWindow.history.forward(); } catch (e) {}
  });

  document.querySelectorAll('.quick-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.preventDefault();
      const url = btn.dataset.url;
      if (url) navigateTo(url);
    });
  });

  // Export functions to window for UI automation and tests
  window.slateShell = {
    tabs: tabs,
    navigateTo: navigateTo,
    createNewTab: createNewTab,
    activateTab: activateTab,
    closeTab: closeTab
  };

  // Initial render
  renderTabs();
  renderViewport(tabs[0]);
})();
