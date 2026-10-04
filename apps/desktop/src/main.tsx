import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import './app/app.css';

const container = document.getElementById('root');
if (container === null) {
  throw new Error('index.html has no element with the id "root"');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
