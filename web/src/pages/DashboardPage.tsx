import { useAuthStore } from '../lib/auth'

export default function DashboardPage() {
  const user = useAuthStore((s) => s.user)

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">Dashboard</h1>
      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
        <p className="text-slate-600">
          Welcome to Casper Platform
          {user ? `, ${user.subject}` : ''}.
        </p>
        <div className="mt-4 grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="bg-slate-50 rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
            <h3 className="text-sm font-medium text-slate-500">Agents</h3>
            <p className="text-2xl font-bold text-slate-900 mt-1">--</p>
          </div>
          <div className="bg-slate-50 rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
            <h3 className="text-sm font-medium text-slate-500">Deployments</h3>
            <p className="text-2xl font-bold text-slate-900 mt-1">--</p>
          </div>
          <div className="bg-slate-50 rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
            <h3 className="text-sm font-medium text-slate-500">API Keys</h3>
            <p className="text-2xl font-bold text-slate-900 mt-1">--</p>
          </div>
        </div>
      </div>
    </div>
  )
}
