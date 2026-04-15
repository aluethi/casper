import { useEffect, useState } from 'react'
import api from '../../lib/api'

interface Tenant {
  id: string; slug: string; display_name: string; status: string;
  created_at: string; updated_at: string;
}

export default function TenantsPage() {
  const [tenants, setTenants] = useState<Tenant[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ slug: '', display_name: '', owner_email: '' })
  const [editId, setEditId] = useState<string | null>(null)
  const [editForm, setEditForm] = useState({ display_name: '', status: '' })

  const load = () => {
    api.get('/api/v1/tenants').then(r => {
      setTenants(r.data.data || r.data)
      setLoading(false)
    }).catch(e => { setError(e.message); setLoading(false) })
  }
  useEffect(load, [])

  const create = () => {
    api.post('/api/v1/tenants', form).then(() => {
      setShowCreate(false)
      setForm({ slug: '', display_name: '', owner_email: '' })
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const startEdit = (t: Tenant) => {
    setEditId(t.id)
    setEditForm({ display_name: t.display_name, status: t.status })
  }

  const saveEdit = () => {
    if (!editId) return
    api.patch(`/api/v1/tenants/${editId}`, editForm).then(() => {
      setEditId(null)
      load()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const suspend = (t: Tenant) => {
    const newStatus = t.status === 'active' ? 'suspended' : 'active'
    api.patch(`/api/v1/tenants/${t.id}`, { status: newStatus }).then(load)
      .catch(e => setError(e.response?.data?.message || e.message))
  }

  if (loading) return <p className="text-gray-500">Loading tenants...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-bold text-gray-900">Tenants</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="px-4 py-2 bg-blue-600 text-white rounded-lg text-sm hover:bg-blue-700">
          + Create Tenant
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-lg text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {showCreate && (
        <div className="mb-6 p-4 bg-white border border-gray-200 rounded-lg space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <input placeholder="Slug (e.g. acme)" value={form.slug} onChange={e => setForm({...form, slug: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input placeholder="Display Name" value={form.display_name} onChange={e => setForm({...form, display_name: e.target.value})} className="border rounded px-3 py-2 text-sm" />
            <input placeholder="Owner email" value={form.owner_email} onChange={e => setForm({...form, owner_email: e.target.value})} className="border rounded px-3 py-2 text-sm" />
          </div>
          <div className="flex gap-2">
            <button onClick={create} className="px-4 py-2 bg-green-600 text-white rounded text-sm hover:bg-green-700">Create</button>
            <button onClick={() => setShowCreate(false)} className="px-4 py-2 bg-gray-200 text-gray-700 rounded text-sm hover:bg-gray-300">Cancel</button>
          </div>
        </div>
      )}

      <table className="w-full bg-white border border-gray-200 rounded-lg overflow-hidden">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Name</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Slug</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Status</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Created</th>
            <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase">Actions</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {tenants.map(t => (
            <tr key={t.id} className="hover:bg-gray-50">
              <td className="px-4 py-3">
                {editId === t.id ? (
                  <input value={editForm.display_name} onChange={e => setEditForm({...editForm, display_name: e.target.value})} className="border rounded px-2 py-1 text-sm w-full" />
                ) : (
                  <span className="text-sm font-medium">{t.display_name}</span>
                )}
              </td>
              <td className="px-4 py-3 text-sm font-mono text-gray-600">{t.slug}</td>
              <td className="px-4 py-3">
                {editId === t.id ? (
                  <select value={editForm.status} onChange={e => setEditForm({...editForm, status: e.target.value})} className="border rounded px-2 py-1 text-sm">
                    <option value="active">active</option>
                    <option value="suspended">suspended</option>
                    <option value="deactivated">deactivated</option>
                  </select>
                ) : (
                  <span className={`px-2 py-1 rounded text-xs font-medium ${t.status === 'active' ? 'bg-green-100 text-green-700' : t.status === 'suspended' ? 'bg-yellow-100 text-yellow-700' : 'bg-red-100 text-red-700'}`}>{t.status}</span>
                )}
              </td>
              <td className="px-4 py-3 text-sm text-gray-600">{new Date(t.created_at).toLocaleDateString()}</td>
              <td className="px-4 py-3">
                {editId === t.id ? (
                  <div className="flex gap-2">
                    <button onClick={saveEdit} className="text-green-600 hover:text-green-800 text-sm font-medium">Save</button>
                    <button onClick={() => setEditId(null)} className="text-gray-500 hover:text-gray-700 text-sm">Cancel</button>
                  </div>
                ) : (
                  <div className="flex gap-3">
                    <button onClick={() => startEdit(t)} className="text-blue-600 hover:text-blue-800 text-sm">Edit</button>
                    <button onClick={() => suspend(t)} className={`text-sm ${t.status === 'active' ? 'text-yellow-600 hover:text-yellow-800' : 'text-green-600 hover:text-green-800'}`}>
                      {t.status === 'active' ? 'Suspend' : 'Activate'}
                    </button>
                  </div>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
