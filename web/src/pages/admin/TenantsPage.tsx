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

  if (loading) return <p className="text-slate-500">Loading tenants...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Tenants</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          + Create Tenant
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-xl ring-1 ring-red-200 text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      {showCreate && (
        <div className="mb-6 p-4 bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <input placeholder="Slug (e.g. acme)" value={form.slug} onChange={e => setForm({...form, slug: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input placeholder="Display Name" value={form.display_name} onChange={e => setForm({...form, display_name: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input placeholder="Owner email" value={form.owner_email} onChange={e => setForm({...form, owner_email: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          </div>
          <div className="flex gap-2">
            <button onClick={create} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">Create</button>
            <button onClick={() => setShowCreate(false)} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">Cancel</button>
          </div>
        </div>
      )}

      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
        <table className="w-full">
          <thead className="bg-slate-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Name</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Slug</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Status</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Created</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {tenants.map(t => (
              <tr key={t.id} className="hover:bg-slate-50">
                <td className="px-4 py-3">
                  {editId === t.id ? (
                    <input value={editForm.display_name} onChange={e => setEditForm({...editForm, display_name: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow w-full" />
                  ) : (
                    <span className="text-sm font-medium">{t.display_name}</span>
                  )}
                </td>
                <td className="px-4 py-3 text-sm font-mono text-slate-600">{t.slug}</td>
                <td className="px-4 py-3">
                  {editId === t.id ? (
                    <select value={editForm.status} onChange={e => setEditForm({...editForm, status: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
                      <option value="active">active</option>
                      <option value="suspended">suspended</option>
                      <option value="deactivated">deactivated</option>
                    </select>
                  ) : (
                    <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${t.status === 'active' ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : t.status === 'suspended' ? 'bg-amber-50 text-amber-700 ring-1 ring-amber-600/20' : 'bg-red-50 text-red-700 ring-1 ring-red-600/20'}`}>{t.status}</span>
                  )}
                </td>
                <td className="px-4 py-3 text-sm text-slate-600">{new Date(t.created_at).toLocaleDateString()}</td>
                <td className="px-4 py-3">
                  {editId === t.id ? (
                    <div className="flex gap-2">
                      <button onClick={saveEdit} className="text-green-600 hover:text-green-500 text-sm font-medium transition-colors">Save</button>
                      <button onClick={() => setEditId(null)} className="text-slate-500 hover:text-slate-700 text-sm font-medium transition-colors">Cancel</button>
                    </div>
                  ) : (
                    <div className="flex gap-3">
                      <button onClick={() => startEdit(t)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
                      <button onClick={() => suspend(t)} className={`text-sm font-medium transition-colors ${t.status === 'active' ? 'text-amber-600 hover:text-amber-500' : 'text-green-600 hover:text-green-500'}`}>
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
    </div>
  )
}
