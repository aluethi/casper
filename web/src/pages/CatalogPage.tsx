import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { Model } from '../types'

const capBadges: { key: keyof Model; label: string }[] = [
  { key: 'cap_chat', label: 'Chat' },
  { key: 'cap_embedding', label: 'Embedding' },
  { key: 'cap_thinking', label: 'Thinking' },
  { key: 'cap_vision', label: 'Vision' },
  { key: 'cap_tool_use', label: 'Tools' },
  { key: 'cap_json_output', label: 'JSON' },
  { key: 'cap_audio_in', label: 'Audio In' },
  { key: 'cap_audio_out', label: 'Audio Out' },
  { key: 'cap_image_gen', label: 'Image Gen' },
]

export default function CatalogPage() {
  const [models, setModels] = useState<(Model & { has_quota?: boolean })[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [providerFilter, setProviderFilter] = useState('')

  useEffect(() => {
    api.get('/api/v1/catalog')
      .then((r) => setModels(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }, [])

  const providers = [...new Set(models.map((m) => m.provider))].sort()
  const filtered = providerFilter ? models.filter((m) => m.provider === providerFilter) : models

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">Model Catalog</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      <div className="mb-4">
        <select
          value={providerFilter}
          onChange={(e) => setProviderFilter(e.target.value)}
          className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow"
        >
          <option value="">All Providers</option>
          {providers.map((p) => <option key={p} value={p}>{p}</option>)}
        </select>
      </div>

      {filtered.length === 0 ? (
        <p className="text-slate-500">No models found.</p>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {filtered.map((m) => (
            <div key={m.id} className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-4">
              <div className="flex items-start justify-between mb-2">
                <div>
                  <h3 className="font-semibold text-slate-900">{m.display_name || m.name}</h3>
                  <p className="text-xs text-slate-500">{m.provider}</p>
                </div>
                {m.has_quota !== undefined && (
                  <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${m.has_quota ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-red-50 text-red-700 ring-1 ring-red-600/20'}`}>
                    {m.has_quota ? 'Quota OK' : 'No Quota'}
                  </span>
                )}
              </div>
              <div className="text-xs text-slate-500 mb-3 space-y-1">
                <p>Context: {m.context_window?.toLocaleString()} tokens</p>
                <p>Max output: {m.max_output_tokens?.toLocaleString()} tokens</p>
                <p>Cost: ${m.cost_per_1k_input}/1k in, ${m.cost_per_1k_output}/1k out</p>
              </div>
              <div className="flex flex-wrap gap-1">
                {capBadges.filter((c) => m[c.key]).map((c) => (
                  <span key={c.key} className="rounded-full px-2 py-0.5 bg-blue-50 text-blue-700 text-xs font-medium ring-1 ring-blue-600/20">
                    {c.label}
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
