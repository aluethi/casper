import { Routes, Route, Navigate } from 'react-router-dom'
import Layout from './components/Layout'
import ProtectedRoute from './components/ProtectedRoute'
import LoginPage from './pages/LoginPage'
import DashboardPage from './pages/DashboardPage'
import AgentListPage from './pages/AgentListPage'
import AgentBuilderPage from './pages/AgentBuilderPage'
import CatalogPage from './pages/CatalogPage'
import DeploymentListPage from './pages/DeploymentListPage'
import ApiKeyListPage from './pages/ApiKeyListPage'
import KnowledgePage from './pages/KnowledgePage'
import MemoryPage from './pages/MemoryPage'
import ConversationListPage from './pages/ConversationListPage'
import UsagePage from './pages/UsagePage'
import AuditPage from './pages/AuditPage'
import UsersPage from './pages/admin/UsersPage'
import SecretsPage from './pages/admin/SecretsPage'
import ModelsPage from './pages/admin/ModelsPage'
import BackendsPage from './pages/admin/BackendsPage'
import QuotasPage from './pages/admin/QuotasPage'
import TenantsPage from './pages/admin/TenantsPage'
import PlaygroundPage from './pages/PlaygroundPage'
import ConnectionsPage from './pages/ConnectionsPage'

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />

      <Route
        element={
          <ProtectedRoute>
            <Layout />
          </ProtectedRoute>
        }
      >
        <Route path="/" element={<DashboardPage />} />
        <Route path="/agents" element={<AgentListPage />} />
        <Route path="/agents/:name" element={<AgentBuilderPage />} />
        <Route path="/catalog" element={<CatalogPage />} />
        <Route path="/deployments" element={<DeploymentListPage />} />
        <Route path="/playground" element={<PlaygroundPage />} />
        <Route path="/keys" element={<ApiKeyListPage />} />
        <Route path="/knowledge" element={<KnowledgePage />} />
        <Route path="/memory" element={<MemoryPage />} />
        <Route path="/conversations" element={<ConversationListPage />} />
        <Route path="/settings/connections" element={<ConnectionsPage />} />
        <Route path="/usage" element={<UsagePage />} />
        <Route path="/audit" element={<AuditPage />} />
        <Route path="/admin/users" element={<UsersPage />} />
        <Route path="/admin/secrets" element={<SecretsPage />} />
        <Route path="/admin/models" element={<ModelsPage />} />
        <Route path="/admin/backends" element={<BackendsPage />} />
        <Route path="/admin/quotas" element={<QuotasPage />} />
        <Route path="/admin/tenants" element={<TenantsPage />} />
      </Route>

      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
