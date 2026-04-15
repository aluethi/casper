import { NavLink, Outlet } from 'react-router-dom'
import { useAuthStore } from '../lib/auth'

const navSections = [
  {
    title: 'Inference',
    links: [
      { to: '/catalog', label: 'Catalog' },
      { to: '/deployments', label: 'Deployments' },
      { to: '/keys', label: 'API Keys' },
    ],
  },
  {
    title: 'Agents',
    links: [
      { to: '/agents', label: 'Agents' },
      { to: '/knowledge', label: 'Knowledge' },
      { to: '/memory', label: 'Memory' },
      { to: '/conversations', label: 'Conversations' },
    ],
  },
  {
    title: 'Settings',
    links: [
      { to: '/admin/users', label: 'Users' },
      { to: '/admin/secrets', label: 'Secrets' },
      { to: '/audit', label: 'Audit' },
      { to: '/usage', label: 'Usage' },
    ],
  },
]

export default function Layout() {
  const user = useAuthStore((s) => s.user)
  const logout = useAuthStore((s) => s.logout)

  return (
    <div className="flex h-screen bg-gray-50">
      {/* Sidebar */}
      <aside className="w-64 bg-white border-r border-gray-200 flex flex-col">
        <div className="p-4 border-b border-gray-200">
          <h1 className="text-xl font-bold text-gray-900">Casper</h1>
        </div>
        <nav className="flex-1 overflow-y-auto p-4 space-y-6">
          {navSections.map((section) => (
            <div key={section.title}>
              <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">
                {section.title}
              </h2>
              <ul className="space-y-1">
                {section.links.map((link) => (
                  <li key={link.to}>
                    <NavLink
                      to={link.to}
                      className={({ isActive }) =>
                        `block px-3 py-2 rounded-md text-sm ${
                          isActive
                            ? 'bg-blue-50 text-blue-700 font-medium'
                            : 'text-gray-700 hover:bg-gray-100'
                        }`
                      }
                    >
                      {link.label}
                    </NavLink>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>
      </aside>

      {/* Main content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Top bar */}
        <header className="h-14 bg-white border-b border-gray-200 flex items-center justify-between px-6">
          <div />
          <div className="flex items-center gap-4">
            {user && (
              <>
                <span className="text-sm text-gray-600">
                  {user.subject}
                </span>
                <span className="text-xs text-gray-400 bg-gray-100 px-2 py-1 rounded">
                  {user.role}
                </span>
                <span className="text-xs text-gray-400">
                  {user.tenant_id}
                </span>
              </>
            )}
            <button
              onClick={() => logout()}
              className="text-sm text-gray-600 hover:text-gray-900"
            >
              Logout
            </button>
          </div>
        </header>

        {/* Page content */}
        <main className="flex-1 overflow-y-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
