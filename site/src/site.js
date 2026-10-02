// Progressive enhancements for the Blocks2Cpp specification page. The page is
// fully readable and navigable without this script.
//
// The page's Content Security Policy enforces Trusted Types, so this file must
// never assign HTML strings (innerHTML and friends). Build DOM nodes instead.
(function () {
  'use strict';

  var THEME_KEY = 'b2c-theme';
  var THEMES = ['system', 'light', 'dark'];
  var THEME_LABELS = { system: 'Theme: System', light: 'Theme: Light', dark: 'Theme: Dark' };

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

  /** Theme button: cycles System → Light → Dark. */
  function setUpThemeToggle() {
    var button = document.getElementById('theme-toggle');
    if (!button) return;
    var current = readTheme();
    button.textContent = THEME_LABELS[current];
    button.hidden = false;
    button.addEventListener('click', function () {
      current = THEMES[(THEMES.indexOf(current) + 1) % THEMES.length];
      applyTheme(current);
      writeTheme(current);
      button.textContent = THEME_LABELS[current];
    });
  }

  /** Highlights the section being read in the sidebar and opens its chapter. */
  function setUpScrollSpy() {
    var toc = document.getElementById('toc-sidebar');
    if (!toc || !('IntersectionObserver' in window)) return;

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

    // The active heading is the last one whose top has scrolled above 30% of the viewport.
    var visible = new Set();
    var observer = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            visible.add(entry.target);
          } else {
            visible.delete(entry.target);
          }
        });
        var candidate = null;
        for (var i = 0; i < headings.length; i += 1) {
          if (headings[i].getBoundingClientRect().top <= window.innerHeight * 0.3) {
            candidate = headings[i];
          } else {
            break;
          }
        }
        activate((candidate || headings[0]).id);
      },
      { rootMargin: '0px 0px -70% 0px', threshold: [0, 1] }
    );
    headings.forEach(function (heading) {
      observer.observe(heading);
    });
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
  setUpScrollSpy();
  setUpCopyButtons();
  setUpMobileToc();
})();
