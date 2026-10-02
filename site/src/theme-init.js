// Applies the reader's saved colour theme before the first paint so the page
// does not flash the wrong theme. Loaded synchronously from <head>.
(function () {
  'use strict';
  try {
    var saved = window.localStorage.getItem('b2c-theme');
    if (saved === 'light' || saved === 'dark') {
      document.documentElement.setAttribute('data-theme', saved);
    }
  } catch (error) {
    // Storage can be unavailable (private mode, blocked site data): follow the OS theme.
  }
})();
