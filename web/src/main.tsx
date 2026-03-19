import './index.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router';
import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '@/lib/query-client';
import { AuthProvider } from '@/contexts/AuthContext';
import { CentrifugoProvider } from '@/contexts/CentrifugoContext';
import { Toaster } from 'sonner';
import { AppShell } from '@/components/AppShell';
import { ProtectedRoute } from '@/components/ProtectedRoute';

import ChatPage from '@/pages/ChatPage';
import AgentsPage from '@/pages/AgentsPage';
import AgentDetailPage from '@/pages/AgentDetailPage';
import AgentEditPage from '@/pages/AgentEditPage';
import AgentNewPage from '@/pages/AgentNewPage';
import CirclesPage from '@/pages/CirclesPage';
import CircleDetailPage from '@/pages/CircleDetailPage';
import InsightsPage from '@/pages/InsightsPage';
import SchedulesPage from '@/pages/SchedulesPage';
import SettingsPage from '@/pages/SettingsPage';
import ToolsPage from '@/pages/ToolsPage';
import MemoryDetailPage from '@/pages/MemoryDetailPage';
import LoginPage from '@/pages/LoginPage';
import AuthCallbackPage from '@/pages/AuthCallbackPage';
import { ForbiddenView } from '@/views/ForbiddenView';
import { NotFoundView } from '@/views/NotFoundView';

const el = document.getElementById('root');
if (!el) throw new Error('Root element not found');

createRoot(el).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <CentrifugoProvider>
          <Toaster position="top-right" theme="dark" />
          <BrowserRouter>
            <Routes>
              <Route path="/login" element={<LoginPage />} />
              <Route path="/auth/callback" element={<AuthCallbackPage />} />
              <Route
                element={
                  <ProtectedRoute>
                    <AppShell />
                  </ProtectedRoute>
                }
              >
                <Route index element={<Navigate to="/chat" replace />} />
                <Route path="chat" element={<ChatPage />} />
                <Route path="agents" element={<AgentsPage />} />
                <Route path="agents/new" element={<AgentNewPage />} />
                <Route path="agents/:id" element={<AgentDetailPage />} />
                <Route path="agents/:id/edit" element={<AgentEditPage />} />
                <Route path="circles" element={<CirclesPage />} />
                <Route path="circles/:id" element={<CircleDetailPage />} />
                <Route path="insights" element={<InsightsPage />} />
                <Route path="schedules" element={<SchedulesPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="tools" element={<ToolsPage />} />
                <Route path="memory/:id" element={<MemoryDetailPage />} />
                <Route path="403" element={<ForbiddenView />} />
              </Route>
              <Route path="*" element={<NotFoundView />} />
            </Routes>
          </BrowserRouter>
        </CentrifugoProvider>
      </AuthProvider>
    </QueryClientProvider>
  </StrictMode>,
);
