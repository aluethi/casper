import { useAuthStore } from '../lib/auth'

export default function DashboardPage() {
  const user = useAuthStore((s) => s.user)

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 mb-4">Dashboard</h1>
      <div className="bg-white rounded-lg border border-gray-200 p-6">
        <p className="text-gray-600">
          Welcome to Casper Platform
          {user ? `, ${user.subject}` : ''}.
        </p>
        <div className="mt-4 grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="bg-gray-50 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-500">Agents</h3>
            <p className="text-2xl font-bold text-gray-900 mt-1">--</p>
          </div>
          <div className="bg-gray-50 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-500">Deployments</h3>
            <p className="text-2xl font-bold text-gray-900 mt-1">--</p>
          </div>
          <div className="bg-gray-50 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-500">API Keys</h3>
            <p className="text-2xl font-bold text-gray-900 mt-1">--</p>
          </div>
        </div>
      </div>
    </div>
  )
}
