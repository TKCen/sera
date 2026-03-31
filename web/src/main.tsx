import './index.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter, Routes, Route } from 'react-router';
import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '@/lib/query-client';
import { AuthProvider } from '@/contexts/AuthContext';
import { CentrifugoProvider } from '@/contexts/CentrifugoContext';
import { Toaster } from 'sonner';
import { AppShell } from '@/components/AppShell';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { ErrorBoundary } from '@/components/ErrorBoundary';

import DashboardPage from '@/app/dashboard/page';
import ChatPage from '@/app/chat/page';
import AgentsPage from '@/app/agents/page';
import AgentDetailPage from '@/app/agents/_id/page';
import AgentEditPage from '@/app/agents/_id/edit/page';
import AgentNewPage from '@/app/agents/new/page';
import CirclesPage from '@/app/circles/page';
import CircleDetailPage from '@/app/circles/_id/page';
import InsightsPage from '@/app/insights/page';
import AuditPage from '@/app/audit/page';
import HealthPage from '@/app/health/page';
import SchedulesPage from '@/app/schedules/page';
import SettingsPage from '@/pages/SettingsPage';
import ToolsPage from '@/pages/ToolsPage';
import MemoryExplorerPage from '@/pages/MemoryExplorerPage';
import MemoryDetailPage from '@/pages/MemoryDetailPage';
import AgentMemoryGraphPage from '@/pages/AgentMemoryGraphPage';
import ChannelsPage from '@/pages/ChannelsPage';
import TemplatesPage from '@/pages/TemplatesPage';
import ProvidersPage from '@/pages/ProvidersPage';
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
                    <ErrorBoundary fallbackMessage="An unexpected error occurred in the application.">
                      <AppShell />
                    </ErrorBoundary>
                  </ProtectedRoute>
                }
              >
                <Route index element={<DashboardPage />} />
                <Route path="chat" element={<ChatPage />} />
                <Route path="agents" element={<AgentsPage />} />
                <Route path="templates" element={<TemplatesPage />} />
                <Route path="agents/new" element={<AgentNewPage />} />
                <Route path="agents/:id" element={<AgentDetailPage />} />
                <Route path="agents/:id/edit" element={<AgentEditPage />} />
                <Route path="agents/:id/memory-graph" element={<AgentMemoryGraphPage />} />
                <Route path="circles" element={<CirclesPage />} />
                <Route path="circles/:id" element={<CircleDetailPage />} />
                <Route path="insights" element={<InsightsPage />} />
                <Route path="audit" element={<AuditPage />} />
                <Route path="health" element={<HealthPage />} />
                <Route path="schedules" element={<SchedulesPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="tools" element={<ToolsPage />} />
                <Route path="channels" element={<ChannelsPage />} />
                <Route path="providers" element={<ProvidersPage />} />
                <Route path="memory" element={<MemoryExplorerPage />} />
                <Route path="memory/:id" element={<MemoryDetailPage />} />
                <Route path="403" element={<ForbiddenView />} />
              </Route>
              <Route path="*" element={<NotFoundView />} />
            </Routes>
          </BrowserRouter>
        </CentrifugoProvider>
      </AuthProvider>
    </QueryClientProvider>
  </StrictMode>
);
