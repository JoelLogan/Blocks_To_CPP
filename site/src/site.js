// Progressive enhancements for the Blocks2Cpp specification page. The page is
// fully readable and navigable without this script.
//
// The page's Content Security Policy enforces Trusted Types, so this file must
// never assign HTML strings (innerHTML and friends). Build DOM nodes instead.
(function () {
  'use strict';

  var THEME_KEY = 'b2c-theme';
  var THEMES = ['system', 'light', 'dark'];
  var THEME_NAMES = { system: 'System', light: 'Light', dark: 'Dark' };

  function readTheme() {
    try {
      var saved = window.localStorage.getItem(THEME_KEY);
      return saved === 'light' || saved === 'dark' ? saved : 'system';
    } catch (error) {
      return 'system';
    }
  }

  function writeTheme(theme) {
    try {
      if (theme === 'system') {
        window.localStorage.removeItem(THEME_KEY);
      } else {
        window.localStorage.setItem(THEME_KEY, theme);
      }
    } catch (error) {
      // Not persisted; the choice still applies until the page is reloaded.
    }
  }

  function applyTheme(theme) {
    if (theme === 'system') {
      document.documentElement.removeAttribute('data-theme');
    } else {
      document.documentElement.setAttribute('data-theme', theme);
    }
  }

  /** Theme button: cycles System → Light → Dark. Its label reads "Theme: <name>". */
  function setUpThemeToggle() {
    var button = document.getElementById('theme-toggle');
    var value = button && button.querySelector('.theme-value');
    if (!button || !value) return;
    var current = readTheme();
    value.textContent = THEME_NAMES[current];
    button.hidden = false;
    button.addEventListener('click', function () {
      current = THEMES[(THEMES.indexOf(current) + 1) % THEMES.length];
      applyTheme(current);
      writeTheme(current);
      value.textContent = THEME_NAMES[current];
    });
  }

  /** Highlights the section being read in the sidebar and opens its chapter. */
  function setUpScrollSpy() {
    var toc = document.getElementById('toc-sidebar');
    if (!toc) return;

    var linksById = new Map();
    toc.querySelectorAll('a[href^="#"]').forEach(function (link) {
      linksById.set(decodeURIComponent(link.getAttribute('href').slice(1)), link);
    });
    var headings = Array.prototype.filter.call(
      document.querySelectorAll('main h2[id], main h3[id]'),
      function (heading) {
        return linksById.has(heading.id);
      }
    );
    if (headings.length === 0) return;
    toc.classList.add('is-enhanced');

    var activeLink = null;
    function activate(id) {
      var link = linksById.get(id);
      if (!link || link === activeLink) return;
      if (activeLink) activeLink.removeAttribute('aria-current');
      link.setAttribute('aria-current', 'location');
      activeLink = link;

      var chapter = link.closest('.toc-list > li');
      toc.querySelectorAll('.toc-list > li.is-current').forEach(function (item) {
        if (item !== chapter) item.classList.remove('is-current');
      });
      if (chapter) chapter.classList.add('is-current');

      // Keep the active entry visible inside the scrollable sidebar.
      var sidebar = toc.closest('.sidebar');
      if (sidebar) {
        var linkBox = link.getBoundingClientRect();
        var barBox = sidebar.getBoundingClientRect();
        if (linkBox.top < barBox.top || linkBox.bottom > barBox.bottom) {
          sidebar.scrollTop += linkBox.top - barBox.top - barBox.height / 3;
        }
      }
    }

    // After a jump to a heading in the contents (a link click or a URL with a #hash),
    // that heading stays current until the reader scrolls by themselves, even if the
    // page cannot scroll it all the way to the top (near the end of the page).
    var pinned = null;
    function pinToHash() {
      var id = decodeURIComponent(window.location.hash.slice(1));
      pinned = linksById.has(id) ? id : null;
    }
    function unpin() {
      pinned = null;
    }

    // Otherwise the current heading is the last one that has reached the reading line
    // just below the sticky top bar (where in-page jumps place headings). This is
    // recomputed from the scroll position at most once per frame, so jumps that skip
    // over headings still update the highlight.
    function update() {
      if (pinned) {
        activate(pinned);
        return;
      }
      var line = (parseFloat(window.getComputedStyle(document.documentElement).scrollPaddingTop) || 72) + 8;
      var candidate = null;
      for (var i = 0; i < headings.length; i += 1) {
        if (headings[i].getBoundingClientRect().top <= line) {
          candidate = headings[i];
        } else {
          break;
        }
      }
      activate((candidate || headings[0]).id);
    }

    var pending = false;
    function schedule() {
      if (pending) return;
      pending = true;
      window.requestAnimationFrame(function () {
        pending = false;
        update();
      });
    }
    window.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule);
    window.addEventListener('hashchange', function () {
      pinToHash();
      schedule();
    });
    ['wheel', 'touchstart', 'keydown', 'mousedown'].forEach(function (type) {
      window.addEventListener(type, unpin, { passive: true });
    });
    pinToHash();
    update();
  }

  /** Keeps the scroll offset for in-page jumps equal to the sticky bar's real height. */
  function setUpTopbarOffset() {
    var bar = document.querySelector('.topbar');
    if (!bar || !('ResizeObserver' in window)) return;
    new ResizeObserver(function () {
      var height = Math.ceil(bar.getBoundingClientRect().height);
      document.documentElement.style.setProperty('--topbar-offset', height + 'px');
    }).observe(bar);
  }

  /**
   * Code blocks and tables are focusable so keyboard users can scroll them. Only
   * keep them in the tab order while their content actually overflows.
   */
  function setUpScrollRegions() {
    if (!('ResizeObserver' in window)) return;
    var regions = document.querySelectorAll('.table-wrap[tabindex], .code-block pre[tabindex], .hero-code[tabindex]');
    function check(region) {
      if (region.scrollWidth > region.clientWidth + 1) {
        region.setAttribute('tabindex', '0');
      } else if (region !== document.activeElement) {
        region.removeAttribute('tabindex');
      }
    }
    var observer = new ResizeObserver(function (entries) {
      entries.forEach(function (entry) {
        check(entry.target);
      });
    });
    regions.forEach(function (region) {
      observer.observe(region);
    });
    // A web font swap can change content width without resizing the box.
    if (document.fonts && document.fonts.ready) {
      document.fonts.ready.then(function () {
        regions.forEach(check);
      });
    }
  }

  /** Adds a Copy button to every code block. */
  function setUpCopyButtons() {
    var status = document.createElement('p');
    status.className = 'visually-hidden';
    status.setAttribute('role', 'status');
    document.body.appendChild(status);

    document.querySelectorAll('.code-block').forEach(function (block) {
      var bar = block.querySelector('.code-bar');
      var code = block.querySelector('pre code');
      if (!bar || !code) return;

      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'code-copy';
      button.textContent = 'Copy';
      var label = bar.querySelector('.code-lang');
      button.setAttribute('aria-label', label ? 'Copy ' + label.textContent + ' code' : 'Copy code');
      bar.appendChild(button);

      var resetTimer = 0;
      function report(message) {
        button.textContent = message;
        status.textContent = message;
        window.clearTimeout(resetTimer);
        resetTimer = window.setTimeout(function () {
          button.textContent = 'Copy';
        }, 2000);
      }

      function selectCode() {
        var range = document.createRange();
        range.selectNodeContents(code);
        var selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
      }

      button.addEventListener('click', function () {
        var text = code.textContent;
        if (navigator.clipboard && window.isSecureContext) {
          navigator.clipboard.writeText(text).then(
            function () {
              report('Copied');
            },
            function () {
              selectCode();
              report('Selected – press Ctrl+C');
            }
          );
        } else {
          selectCode();
          report('Selected – press Ctrl+C');
        }
      });
    });
  }

  /** Closes the mobile contents menu after a link in it is chosen. */
  function setUpMobileToc() {
    var menu = document.querySelector('.toc-mobile');
    if (!menu) return;
    menu.addEventListener('click', function (event) {
      if (event.target instanceof Element && event.target.closest('a[href^="#"]')) {
        menu.open = false;
      }
    });
  }

  setUpThemeToggle();
  setUpTopbarOffset();
  setUpScrollSpy();
  setUpScrollRegions();
  setUpCopyButtons();
  setUpMobileToc();
})();
