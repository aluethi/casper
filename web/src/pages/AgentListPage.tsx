import { useState, useEffect } from 'react'
import { Link } from 'react-router-dom'
import api from '../lib/api'
import type { Agent, Deployment, AvailableModel } from '../types'

export default function AgentListPage() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', display_name: '', description: '', model_deployment: '' })
  const [saving, setSaving] = useState(false)
  const [deployments, setDeployments] = useState<Deployment[]>([])
  const [models, setModels] = useState<AvailableModel[]>([])

  const load = () => {
    setLoading(true)
    api.get('/api/v1/agents')
      .then((r) => setAgents(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(() => {
    load()
    Promise.all([
      api.get('/api/v1/deployments?per_page=100'),
      api.get('/api/v1/deployments/available-models'),
    ]).then(([depRes, modRes]) => {
      setDeployments((depRes.data.data ?? depRes.data).filter((d: Deployment) => d.is_active))
      setModels(modRes.data)
    }).catch(() => {})
  }, [])

  const create = async () => {
    setSaving(true)
    setError('')
    try {
      await api.post('/api/v1/agents', form)
      setShowForm(false)
      setForm({ name: '', display_name: '', description: '', model_deployment: '' })
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Agents</h1>
        <button onClick={() => setShowForm(!showForm)}
          className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
          {showForm ? 'Cancel' : 'Create Agent'}
        </button>
      </div>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      {showForm && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4 mb-4 space-y-3">
          <input placeholder="Name (slug)" value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          <input placeholder="Display Name" value={form.display_name}
            onChange={(e) => setForm({ ...form, display_name: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" />
          <textarea placeholder="Description" value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" rows={2} />
          <select value={form.model_deployment}
            onChange={(e) => setForm({ ...form, model_deployment: e.target.value })}
            className="w-full rounded-lg ring-1 ring-slate-300 bg-white px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
            <option value="">Select a deployment...</option>
            {deployments.map(d => {
              const m = models.find(x => x.id === d.model_id)
              return <option key={d.slug} value={d.slug}>{d.name} ({d.slug}){m ? ` \u2014 ${m.display_name}` : ''}</option>
            })}
          </select>
          <button onClick={create} disabled={saving}
            className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
            {saving ? 'Creating...' : 'Create'}
          </button>
        </div>
      )}

      {agents.length === 0 ? (
        <p className="text-slate-500">No agents yet.</p>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {agents.map((a) => (
            <Link key={a.id} to={`/agents/${a.name}`}
              className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4 hover:ring-blue-300 transition-colors">
              <div className="flex items-start justify-between mb-2">
                <h3 className="font-semibold text-slate-900">{a.display_name || a.name}</h3>
                <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${a.status === 'active' ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-slate-50 text-slate-600 ring-1 ring-slate-600/20'}`}>
                  {a.status}
                </span>
              </div>
              {a.description && <p className="text-sm text-slate-500 mb-2">{a.description}</p>}
              <div className="text-xs text-slate-400 space-y-0.5">
                <p>Model: {a.model_deployment}</p>
                <p>Version: {a.version}</p>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
