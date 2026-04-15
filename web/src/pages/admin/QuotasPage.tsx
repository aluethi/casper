import { useEffect, useState } from 'react'
import api from '../../lib/api'

interface Quota {
  tenant_id: string; model_id: string; requests_per_minute: number;
  tokens_per_day: number; cache_tokens_per_day: number;
  allocated_by: string; allocated_at: string;
}

interface Tenant { id: string; slug: string; display_name: string }
interface Model { id: string; name: string; display_name: string }

export default function QuotasPage() {
  const [tenants, setTenants] = useState<Tenant[]>([])
  const [models, setModels] = useState<Model[]>([])
  const [selectedTenant, setSelectedTenant] = useState('')
  const [quotas, setQuotas] = useState<Quota[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ model_id: '', requests_per_minute: 100, tokens_per_day: 1000000, cache_tokens_per_day: 0 })
  const [editModelId, setEditModelId] = useState<string | null>(null)
  const [editForm, setEditForm] = useState({ requests_per_minute: 0, tokens_per_day: 0, cache_tokens_per_day: 0 })

  useEffect(() => {
    Promise.all([
      api.get('/api/v1/tenants'),
      api.get('/api/v1/models'),
    ]).then(([t, m]) => {
      const tenantList = t.data.data || t.data
      const modelList = m.data.data || m.data
      setTenants(tenantList)
      setModels(modelList)
      if (tenantList.length > 0) setSelectedTenant(tenantList[0].id)
      setLoading(false)
    }).catch(e => { setError(e.message); setLoading(false) })
  }, [])

  const loadQuotas = () => {
    if (!selectedTenant) return
    api.get(`/api/v1/tenants/${selectedTenant}/quotas`).then(r => {
      setQuotas(r.data.data || r.data || [])
    }).catch(() => setQuotas([]))
  }
  useEffect(loadQuotas, [selectedTenant])

  const allocate = () => {
    api.post(`/api/v1/tenants/${selectedTenant}/quotas`, form).then(() => {
      setShowCreate(false)
      loadQuotas()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const startEdit = (q: Quota) => {
    setEditModelId(q.model_id)
    setEditForm({ requests_per_minute: q.requests_per_minute, tokens_per_day: q.tokens_per_day, cache_tokens_per_day: q.cache_tokens_per_day })
  }

  const saveEdit = () => {
    if (!editModelId) return
    api.patch(`/api/v1/tenants/${selectedTenant}/quotas/${editModelId}`, editForm).then(() => {
      setEditModelId(null)
      loadQuotas()
    }).catch(e => setError(e.response?.data?.message || e.message))
  }

  const deleteQuota = (modelId: string) => {
    if (!confirm('Remove this quota allocation?')) return
    api.delete(`/api/v1/tenants/${selectedTenant}/quotas/${modelId}`).then(() => {
      setQuotas(quotas.filter(q => q.model_id !== modelId))
    })
  }

  const modelName = (id: string) => models.find(m => m.id === id)?.display_name || id.slice(0, 8)

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <div className="flex justify-between items-center mb-6">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Model Quotas</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          + Allocate Quota
        </button>
      </div>
      {error && <div className="mb-4 p-3 bg-red-50 text-red-700 rounded-xl ring-1 ring-red-200 text-sm">{error}<button onClick={() => setError('')} className="ml-2 underline">dismiss</button></div>}

      <div className="mb-4">
        <label className="text-sm font-medium text-slate-700 mr-2">Tenant:</label>
        <select value={selectedTenant} onChange={e => setSelectedTenant(e.target.value)} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
          {tenants.map(t => <option key={t.id} value={t.id}>{t.display_name} ({t.slug})</option>)}
        </select>
      </div>

      {showCreate && (
        <div className="mb-6 p-4 bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm space-y-3">
          <div className="grid grid-cols-4 gap-3">
            <select value={form.model_id} onChange={e => setForm({...form, model_id: e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
              <option value="">Select model...</option>
              {models.map(m => <option key={m.id} value={m.id}>{m.display_name}</option>)}
            </select>
            <input type="number" placeholder="RPM" value={form.requests_per_minute} onChange={e => setForm({...form, requests_per_minute: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" placeholder="Tokens/day" value={form.tokens_per_day} onChange={e => setForm({...form, tokens_per_day: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
            <input type="number" placeholder="Cache tokens/day" value={form.cache_tokens_per_day} onChange={e => setForm({...form, cache_tokens_per_day: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          </div>
          <div className="flex gap-2">
            <button onClick={allocate} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">Allocate</button>
            <button onClick={() => setShowCreate(false)} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">Cancel</button>
          </div>
        </div>
      )}

      <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
        <table className="w-full">
          <thead className="bg-slate-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Model</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">RPM</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Tokens/day</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Cache/day</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Allocated by</th>
              <th className="px-4 py-3 text-left text-xs font-medium text-slate-500 uppercase tracking-wide">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {quotas.length === 0 && (
              <tr><td colSpan={6} className="px-4 py-8 text-center text-slate-500">No quotas allocated for this tenant.</td></tr>
            )}
            {quotas.map(q => (
              <tr key={q.model_id} className="hover:bg-slate-50">
                <td className="px-4 py-3 text-sm font-medium">{modelName(q.model_id)}</td>
                <td className="px-4 py-3 text-sm">
                  {editModelId === q.model_id
                    ? <input type="number" value={editForm.requests_per_minute} onChange={e => setEditForm({...editForm, requests_per_minute: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow w-24" />
                    : q.requests_per_minute.toLocaleString()
                  }
                </td>
                <td className="px-4 py-3 text-sm">
                  {editModelId === q.model_id
                    ? <input type="number" value={editForm.tokens_per_day} onChange={e => setEditForm({...editForm, tokens_per_day: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow w-32" />
                    : q.tokens_per_day.toLocaleString()
                  }
                </td>
                <td className="px-4 py-3 text-sm">
                  {editModelId === q.model_id
                    ? <input type="number" value={editForm.cache_tokens_per_day} onChange={e => setEditForm({...editForm, cache_tokens_per_day: +e.target.value})} className="rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow w-32" />
                    : q.cache_tokens_per_day.toLocaleString()
                  }
                </td>
                <td className="px-4 py-3 text-sm text-slate-600">{q.allocated_by}</td>
                <td className="px-4 py-3">
                  {editModelId === q.model_id ? (
                    <div className="flex gap-2">
                      <button onClick={saveEdit} className="text-green-600 hover:text-green-500 text-sm font-medium transition-colors">Save</button>
                      <button onClick={() => setEditModelId(null)} className="text-slate-500 hover:text-slate-700 text-sm font-medium transition-colors">Cancel</button>
                    </div>
                  ) : (
                    <div className="flex gap-2">
                      <button onClick={() => startEdit(q)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
                      <button onClick={() => deleteQuota(q.model_id)} className="text-red-600 hover:text-red-500 text-sm font-medium transition-colors">Remove</button>
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
