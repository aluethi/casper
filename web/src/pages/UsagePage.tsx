import { useState, useEffect } from 'react'
import api from '../lib/api'
import type { UsageRecord } from '../types'

export default function UsagePage() {
  const [records, setRecords] = useState<UsageRecord[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  useEffect(() => {
    api.get('/api/v1/usage/events')
      .then((r) => setRecords(r.data.data ?? r.data))
      .catch(() =>
        api.get('/api/v1/usage')
          .then((r) => setRecords(r.data.data ?? r.data))
          .catch((e) => setError(e.response?.data?.message ?? e.message))
      )
      .finally(() => setLoading(false))
  }, [])

  const totalInput = records.reduce((s, r) => s + r.input_tokens, 0)
  const totalOutput = records.reduce((s, r) => s + r.output_tokens, 0)
  const totalCost = records.reduce((s, r) => s + r.cost, 0)
  const totalRequests = records.length

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">Usage</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
          <h3 className="text-sm font-medium text-slate-500">Total Requests</h3>
          <p className="text-2xl font-bold text-slate-900 mt-1">{totalRequests.toLocaleString()}</p>
        </div>
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
          <h3 className="text-sm font-medium text-slate-500">Input Tokens</h3>
          <p className="text-2xl font-bold text-slate-900 mt-1">{totalInput.toLocaleString()}</p>
        </div>
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
          <h3 className="text-sm font-medium text-slate-500">Output Tokens</h3>
          <p className="text-2xl font-bold text-slate-900 mt-1">{totalOutput.toLocaleString()}</p>
        </div>
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-lg p-4">
          <h3 className="text-sm font-medium text-slate-500">Total Cost</h3>
          <p className="text-2xl font-bold text-slate-900 mt-1">${totalCost.toFixed(4)}</p>
        </div>
      </div>

      {records.length === 0 ? (
        <p className="text-slate-500">No usage data yet.</p>
      ) : (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-left text-slate-600">
              <tr>
                <th className="px-4 py-3">Date</th><th className="px-4 py-3">Model</th>
                <th className="px-4 py-3 text-right">Input</th><th className="px-4 py-3 text-right">Output</th>
                <th className="px-4 py-3 text-right">Cache Read</th><th className="px-4 py-3 text-right">Cache Write</th>
                <th className="px-4 py-3 text-right">Cost</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {records.map((r) => (
                <tr key={r.id} className="hover:bg-slate-50">
                  <td className="px-4 py-3 text-slate-500">{new Date(r.created_at).toLocaleString()}</td>
                  <td className="px-4 py-3 text-slate-700">{r.model_id}</td>
                  <td className="px-4 py-3 text-right text-slate-500">{r.input_tokens.toLocaleString()}</td>
                  <td className="px-4 py-3 text-right text-slate-500">{r.output_tokens.toLocaleString()}</td>
                  <td className="px-4 py-3 text-right text-slate-500">{r.cache_read_tokens.toLocaleString()}</td>
                  <td className="px-4 py-3 text-right text-slate-500">{r.cache_write_tokens.toLocaleString()}</td>
                  <td className="px-4 py-3 text-right font-medium text-slate-900">${r.cost.toFixed(4)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
