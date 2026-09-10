import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClientProvider } from '@tanstack/react-query';
import { App } from './app';
import { createQueryClient } from './lib/query-client';
import './styles.css';

try { sessionStorage.removeItem('steward.connection-token'); } catch { /* Storage may be disabled. */ }
const root = document.getElementById('root');
if (!root) throw new Error('Missing application root');
createRoot(root).render(<StrictMode><QueryClientProvider client={createQueryClient()}><App /></QueryClientProvider></StrictMode>);
