import { useState, useEffect } from 'react'
import api from '../../lib/api'

interface UserRecord {
  id: string
  email: string
  role: string
  scopes: string[]
  last_login_at: string | null
  created_at: string
}

export default function UsersPage() {
  const [users, setUsers] = useState<UserRecord[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ email: '', role: 'member', scopes: '' })
  const [saving, setSaving] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [editForm, setEditForm] = useState({ role: '', scopes: '' })

  const load = () => {
    setLoading(true)
    api.get('/api/v1/users')
      .then((r) => setUsers(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const create = async () => {
    setSaving(true)
    setError('')
    try {
      const scopes = form.scopes.split(',').map((s) => s.trim()).filter(Boolean)
      await api.post('/api/v1/users', { email: form.email, role: form.role, scopes })
      setShowForm(false)
      setForm({ email: '', role: 'member', scopes: '' })
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const startEdit = (u: UserRecord) => {
    setEditing(u.id)
    setEditForm({ role: u.role, scopes: u.scopes?.join(', ') ?? '' })
  }

  const saveEdit = async (id: string) => {
    setSaving(true)
    setError('')
    try {
      const scopes = editForm.scopes.split(',').map((s) => s.trim()).filter(Boolean)
      await api.patch(`/api/v1/users/${id}`, { role: editForm.role, scopes })
      setEditing(null)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this user?')) return
    try {
      await api.delete(`/api/v1/users/${id}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Users</h1>
        <button onClick={() => setShowForm(!showForm)}
          className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          {showForm ? 'Cancel' : 'Add User'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      {showForm && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4 mb-4 space-y-3">
          <input placeholder="Email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          <select value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
            <option value="member">Member</option>
            <option value="admin">Admin</option>
            <option value="owner">Owner</option>
          </select>
          <input placeholder="Scopes (comma-separated)" value={form.scopes}
            onChange={(e) => setForm({ ...form, scopes: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          <button onClick={create} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
            {saving ? 'Creating...' : 'Add User'}
          </button>
        </div>
      )}

      {users.length === 0 ? (
        <p className="text-slate-500">No users yet.</p>
      ) : (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-left text-slate-600">
              <tr>
                <th className="px-4 py-3">Email</th><th className="px-4 py-3">Role</th>
                <th className="px-4 py-3">Scopes</th><th className="px-4 py-3">Last Login</th>
                <th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {users.map((u) => (
                <tr key={u.id} className="hover:bg-slate-50">
                  <td className="px-4 py-3 font-medium text-slate-900">{u.email}</td>
                  <td className="px-4 py-3">
                    {editing === u.id ? (
                      <select value={editForm.role} onChange={(e) => setEditForm({ ...editForm, role: e.target.value })}
                        className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-xs shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
                        <option value="member">Member</option>
                        <option value="admin">Admin</option>
                        <option value="owner">Owner</option>
                      </select>
                    ) : (
                      <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${u.role === 'admin' || u.role === 'owner' ? 'bg-purple-50 text-purple-700 ring-1 ring-purple-600/20' : 'bg-slate-50 text-slate-600 ring-1 ring-slate-600/20'}`}>
                        {u.role}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    {editing === u.id ? (
                      <input value={editForm.scopes} onChange={(e) => setEditForm({ ...editForm, scopes: e.target.value })}
                        className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-xs shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow w-full" />
                    ) : (
                      <div className="flex flex-wrap gap-1">
                        {u.scopes?.map((s) => <span key={s} className="rounded-full px-2.5 py-0.5 text-xs font-medium bg-slate-50 text-slate-600 ring-1 ring-slate-600/20">{s}</span>)}
                      </div>
                    )}
                  </td>
                  <td className="px-4 py-3 text-slate-500">{u.last_login_at ? new Date(u.last_login_at).toLocaleString() : 'Never'}</td>
                  <td className="px-4 py-3 text-right space-x-2">
                    {editing === u.id ? (
                      <>
                        <button onClick={() => saveEdit(u.id)} disabled={saving} className="text-blue-600 hover:text-blue-500 text-xs font-medium transition-colors">Save</button>
                        <button onClick={() => setEditing(null)} className="text-slate-500 text-xs font-medium hover:text-slate-700 transition-colors">Cancel</button>
                      </>
                    ) : (
                      <>
                        <button onClick={() => startEdit(u)} className="text-blue-600 hover:text-blue-500 text-xs font-medium transition-colors">Edit</button>
                        <button onClick={() => remove(u.id)} className="text-red-600 hover:text-red-500 text-xs font-medium transition-colors">Delete</button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
