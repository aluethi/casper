import { NavLink, Outlet } from 'react-router-dom'
import { useAuthStore } from '../lib/auth'

const navSections = [
  {
    title: 'Inference',
    links: [
      { to: '/catalog', label: 'Catalog' },
      { to: '/deployments', label: 'Deployments' },
      { to: '/playground', label: 'Playground' },
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
      { to: '/settings/connections', label: 'Connections' },
      { to: '/admin/users', label: 'Users' },
      { to: '/admin/secrets', label: 'Secrets' },
      { to: '/audit', label: 'Audit' },
      { to: '/usage', label: 'Usage' },
    ],
  },
  {
    title: 'Platform Admin',
    links: [
      { to: '/admin/tenants', label: 'Tenants' },
      { to: '/admin/models', label: 'Models' },
      { to: '/admin/backends', label: 'Backends' },
      { to: '/admin/quotas', label: 'Quotas' },
    ],
  },
]

function getInitials(name: string) {
  return name
    .split(/[@.\s]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((s) => s[0].toUpperCase())
    .join('')
}

export default function Layout() {
  const user = useAuthStore((s) => s.user)
  const logout = useAuthStore((s) => s.logout)

  return (
    <div className="flex h-screen bg-slate-50">
      {/* Sidebar */}
      <aside className="w-64 bg-slate-900 flex flex-col">
        <div className="p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-full bg-blue-600">
              <svg className="h-4 w-4 text-white" fill="currentColor" viewBox="0 0 20 20">
                <path d="M10 2a6 6 0 00-6 6v3.586l-.707.707A1 1 0 004 14h12a1 1 0 00.707-1.707L16 11.586V8a6 6 0 00-6-6z" />
              </svg>
            </div>
            <h1 className="font-display text-lg font-semibold text-white tracking-tight">Casper</h1>
          </div>
        </div>
        <nav className="flex-1 overflow-y-auto px-3 py-2 space-y-6">
          {navSections.map((section) => (
            <div key={section.title}>
              <h2 className="px-3 text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">
                {section.title}
              </h2>
              <ul className="space-y-0.5">
                {section.links.map((link) => (
                  <li key={link.to}>
                    <NavLink
                      to={link.to}
                      className={({ isActive }) =>
                        `block px-3 py-2 rounded-lg text-sm transition-colors ${
                          isActive
                            ? 'bg-blue-600 text-white font-medium'
                            : 'text-slate-300 hover:text-white hover:bg-white/5'
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

        {/* Bottom user section */}
        {user && (
          <div className="border-t border-white/10 p-4">
            <div className="flex items-center gap-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-full bg-blue-600 text-sm font-medium text-white">
                {getInitials(user.subject)}
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium text-white truncate">{user.subject}</p>
                <span className="inline-block rounded-full bg-white/10 px-2 py-0.5 text-xs text-slate-300">
                  {user.role}
                </span>
              </div>
            </div>
            <button
              onClick={() => logout()}
              className="mt-3 w-full rounded-lg px-3 py-1.5 text-sm text-slate-400 hover:text-white hover:bg-white/5 transition-colors text-left"
            >
              Sign out
            </button>
          </div>
        )}
      </aside>

      {/* Main content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Top bar */}
        <header className="h-14 bg-white shadow-sm flex items-center px-8">
        </header>

        {/* Page content */}
        <main className="flex-1 overflow-y-auto p-8">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
