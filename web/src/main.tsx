import './index.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter, Routes, Route } from 'react-router';
import { QueryClientProvider } from '@tanstack/react-query';
import { Toaster } from 'sonner';
import { queryClient } from '@/lib/query-client';
import { Layout } from '@/components/Layout';
import { HomeView } from '@/views/HomeView';

const el = document.getElementById('root');
if (!el) throw new Error('Root element not found');

createRoot(el).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <Toaster position="top-right" theme="dark" />
      <BrowserRouter>
        <Routes>
          <Route element={<Layout />}>
            <Route index element={<HomeView />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
