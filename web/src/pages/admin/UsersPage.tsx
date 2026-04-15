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
    api.get('/users')
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
      await api.post('/users', { email: form.email, role: form.role, scopes })
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
      await api.patch(`/users/${id}`, { role: editForm.role, scopes })
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
      await api.delete(`/users/${id}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="text-2xl font-bold text-gray-900">Users</h1>
        <button onClick={() => setShowForm(!showForm)}
          className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700">
          {showForm ? 'Cancel' : 'Add User'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      {showForm && (
        <div className="bg-white rounded-lg border border-gray-200 p-4 mb-4 space-y-3">
          <input placeholder="Email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <select value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm">
            <option value="member">Member</option>
            <option value="admin">Admin</option>
            <option value="owner">Owner</option>
          </select>
          <input placeholder="Scopes (comma-separated)" value={form.scopes}
            onChange={(e) => setForm({ ...form, scopes: e.target.value })}
            className="w-full border border-gray-300 rounded px-3 py-2 text-sm" />
          <button onClick={create} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded text-sm hover:bg-blue-700 disabled:opacity-50">
            {saving ? 'Creating...' : 'Add User'}
          </button>
        </div>
      )}

      {users.length === 0 ? (
        <p className="text-gray-500">No users yet.</p>
      ) : (
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-left text-gray-600">
              <tr>
                <th className="px-4 py-3">Email</th><th className="px-4 py-3">Role</th>
                <th className="px-4 py-3">Scopes</th><th className="px-4 py-3">Last Login</th>
                <th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {users.map((u) => (
                <tr key={u.id} className="hover:bg-gray-50">
                  <td className="px-4 py-3 font-medium text-gray-900">{u.email}</td>
                  <td className="px-4 py-3">
                    {editing === u.id ? (
                      <select value={editForm.role} onChange={(e) => setEditForm({ ...editForm, role: e.target.value })}
                        className="border border-gray-300 rounded px-2 py-1 text-xs">
                        <option value="member">Member</option>
                        <option value="admin">Admin</option>
                        <option value="owner">Owner</option>
                      </select>
                    ) : (
                      <span className={`text-xs px-2 py-0.5 rounded-full ${u.role === 'admin' || u.role === 'owner' ? 'bg-purple-100 text-purple-700' : 'bg-gray-100 text-gray-600'}`}>
                        {u.role}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    {editing === u.id ? (
                      <input value={editForm.scopes} onChange={(e) => setEditForm({ ...editForm, scopes: e.target.value })}
                        className="border border-gray-300 rounded px-2 py-1 text-xs w-full" />
                    ) : (
                      <div className="flex flex-wrap gap-1">
                        {u.scopes?.map((s) => <span key={s} className="text-xs bg-gray-100 text-gray-600 px-2 py-0.5 rounded">{s}</span>)}
                      </div>
                    )}
                  </td>
                  <td className="px-4 py-3 text-gray-500">{u.last_login_at ? new Date(u.last_login_at).toLocaleString() : 'Never'}</td>
                  <td className="px-4 py-3 text-right space-x-2">
                    {editing === u.id ? (
                      <>
                        <button onClick={() => saveEdit(u.id)} disabled={saving} className="text-blue-600 text-xs hover:text-blue-800">Save</button>
                        <button onClick={() => setEditing(null)} className="text-gray-500 text-xs hover:text-gray-700">Cancel</button>
                      </>
                    ) : (
                      <>
                        <button onClick={() => startEdit(u)} className="text-blue-600 text-xs hover:text-blue-800">Edit</button>
                        <button onClick={() => remove(u.id)} className="text-red-600 text-xs hover:text-red-800">Delete</button>
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
