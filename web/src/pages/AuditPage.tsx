import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { AuditEntry } from '../types'

export default function AuditPage() {
  const [entries, setEntries] = useState<AuditEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [actionFilter, setActionFilter] = useState('')
  const [actorFilter, setActorFilter] = useState('')
  const [dateFrom, setDateFrom] = useState('')
  const [dateTo, setDateTo] = useState('')

  const load = () => {
    setLoading(true)
    const params: Record<string, string> = {}
    if (actionFilter) params.action = actionFilter
    if (actorFilter) params.actor = actorFilter
    if (dateFrom) params.from = dateFrom
    if (dateTo) params.to = dateTo
    api.get('/api/v1/audit', { params })
      .then((r) => setEntries(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const actions = [...new Set(entries.map((e) => e.action))].sort()
  const actors = [...new Set(entries.map((e) => e.actor))].sort()

  const filtered = entries.filter((e) =>
    (!actionFilter || e.action === actionFilter) && (!actorFilter || e.actor === actorFilter)
  )

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">Audit Log</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      <div className="flex flex-wrap gap-3 mb-4">
        <select value={actionFilter} onChange={(e) => setActionFilter(e.target.value)}
          className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
          <option value="">All Actions</option>
          {actions.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
        <select value={actorFilter} onChange={(e) => setActorFilter(e.target.value)}
          className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow">
          <option value="">All Actors</option>
          {actors.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
        <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)}
          className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" placeholder="From" />
        <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)}
          className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow" placeholder="To" />
        <button onClick={load} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">
          Apply
        </button>
      </div>

      {filtered.length === 0 ? (
        <p className="text-slate-500">No audit entries found.</p>
      ) : (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-left text-slate-600">
              <tr>
                <th className="px-4 py-3">Timestamp</th><th className="px-4 py-3">Actor</th>
                <th className="px-4 py-3">Action</th><th className="px-4 py-3">Resource</th>
                <th className="px-4 py-3">Resource ID</th><th className="px-4 py-3">Detail</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {filtered.map((e) => (
                <tr key={e.id} className="hover:bg-slate-50">
                  <td className="px-4 py-3 text-slate-500 whitespace-nowrap">{new Date(e.created_at).toLocaleString()}</td>
                  <td className="px-4 py-3 text-slate-700">{e.actor}</td>
                  <td className="px-4 py-3">
                    <span className="rounded-full px-2.5 py-0.5 text-xs font-medium bg-slate-50 text-slate-700 ring-1 ring-slate-600/20">{e.action}</span>
                  </td>
                  <td className="px-4 py-3 text-slate-500">{e.resource_type}</td>
                  <td className="px-4 py-3 text-slate-500 font-mono text-xs">{e.resource_id}</td>
                  <td className="px-4 py-3 text-slate-400 text-xs max-w-xs truncate">
                    {Object.keys(e.detail).length > 0 ? JSON.stringify(e.detail) : '--'}
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
