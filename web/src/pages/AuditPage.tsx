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

  if (loading) return <p className="text-gray-500">Loading...</p>

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 mb-4">Audit Log</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded mb-4">{error}</div>}

      <div className="flex flex-wrap gap-3 mb-4">
        <select value={actionFilter} onChange={(e) => setActionFilter(e.target.value)}
          className="border border-gray-300 rounded px-3 py-2 text-sm">
          <option value="">All Actions</option>
          {actions.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
        <select value={actorFilter} onChange={(e) => setActorFilter(e.target.value)}
          className="border border-gray-300 rounded px-3 py-2 text-sm">
          <option value="">All Actors</option>
          {actors.map((a) => <option key={a} value={a}>{a}</option>)}
        </select>
        <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)}
          className="border border-gray-300 rounded px-3 py-2 text-sm" placeholder="From" />
        <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)}
          className="border border-gray-300 rounded px-3 py-2 text-sm" placeholder="To" />
        <button onClick={load} className="bg-gray-100 text-gray-700 px-4 py-2 rounded text-sm hover:bg-gray-200">
          Apply
        </button>
      </div>

      {filtered.length === 0 ? (
        <p className="text-gray-500">No audit entries found.</p>
      ) : (
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 text-left text-gray-600">
              <tr>
                <th className="px-4 py-3">Timestamp</th><th className="px-4 py-3">Actor</th>
                <th className="px-4 py-3">Action</th><th className="px-4 py-3">Resource</th>
                <th className="px-4 py-3">Resource ID</th><th className="px-4 py-3">Detail</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {filtered.map((e) => (
                <tr key={e.id} className="hover:bg-gray-50">
                  <td className="px-4 py-3 text-gray-500 whitespace-nowrap">{new Date(e.created_at).toLocaleString()}</td>
                  <td className="px-4 py-3 text-gray-700">{e.actor}</td>
                  <td className="px-4 py-3">
                    <span className="text-xs bg-gray-100 text-gray-700 px-2 py-0.5 rounded">{e.action}</span>
                  </td>
                  <td className="px-4 py-3 text-gray-500">{e.resource_type}</td>
                  <td className="px-4 py-3 text-gray-500 font-mono text-xs">{e.resource_id}</td>
                  <td className="px-4 py-3 text-gray-400 text-xs max-w-xs truncate">
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
