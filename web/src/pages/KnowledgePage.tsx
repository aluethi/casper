import { useState, useEffect, useRef } from 'react'
import api from '../lib/api'
import type { KnowledgeDocument } from '../types'

export default function KnowledgePage() {
  const [docs, setDocs] = useState<KnowledgeDocument[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [uploading, setUploading] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [searchResults, setSearchResults] = useState<{ chunk: string; score: number; document_id: string }[] | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  const load = () => {
    setLoading(true)
    api.get('/api/v1/knowledge')
      .then((r) => setDocs(r.data.data ?? r.data))
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  const upload = async () => {
    const file = fileRef.current?.files?.[0]
    if (!file) return
    setUploading(true)
    setError('')
    try {
      const fd = new FormData()
      fd.append('file', file)
      await api.post('/api/v1/knowledge', fd, { headers: { 'Content-Type': 'multipart/form-data' } })
      if (fileRef.current) fileRef.current.value = ''
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setUploading(false)
    }
  }

  const search = async () => {
    if (!searchQuery.trim()) return
    try {
      const res = await api.post('/api/v1/knowledge/search', { query: searchQuery })
      setSearchResults(res.data.results ?? res.data.data ?? res.data)
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this document?')) return
    try {
      await api.delete(`/api/v1/knowledge/${id}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <h1 className="font-display text-3xl tracking-tight text-slate-900 mb-4">Knowledge Base</h1>
      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      <div className="flex flex-wrap gap-4 mb-4">
        <div className="flex gap-2 items-center">
          <input ref={fileRef} type="file" className="text-sm" />
          <button onClick={upload} disabled={uploading}
            className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
            {uploading ? 'Uploading...' : 'Upload'}
          </button>
        </div>
        <div className="flex gap-2 items-center">
          <input placeholder="Search knowledge..." value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') search() }}
            className="rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow w-64" />
          <button onClick={search} className="rounded-full text-sm font-medium text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 px-4 py-2 transition-colors">Search</button>
          {searchResults && (
            <button onClick={() => setSearchResults(null)} className="text-slate-500 text-sm font-medium hover:text-slate-700 transition-colors">Clear</button>
          )}
        </div>
      </div>

      {searchResults && (
        <div className="bg-blue-50 rounded-2xl ring-1 ring-blue-200 shadow-sm p-4 mb-4">
          <h3 className="font-medium text-blue-800 mb-2">Search Results ({searchResults.length})</h3>
          {searchResults.length === 0 ? (
            <p className="text-sm text-blue-600">No results found.</p>
          ) : (
            <div className="space-y-2">
              {searchResults.map((r, i) => (
                <div key={i} className="bg-white rounded-lg p-3 text-sm">
                  <p className="text-slate-700 whitespace-pre-wrap">{r.chunk}</p>
                  <p className="text-xs text-slate-400 mt-1">Score: {r.score?.toFixed(3)} | Doc: {r.document_id}</p>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {docs.length === 0 ? (
        <p className="text-slate-500">No documents yet.</p>
      ) : (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-left text-slate-600">
              <tr>
                <th className="px-4 py-3">Filename</th><th className="px-4 py-3">Type</th>
                <th className="px-4 py-3">Chunks</th><th className="px-4 py-3">Size</th>
                <th className="px-4 py-3">Status</th><th className="px-4 py-3">Created</th><th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {docs.map((d) => (
                <tr key={d.id} className="hover:bg-slate-50">
                  <td className="px-4 py-3 font-medium text-slate-900">{d.filename}</td>
                  <td className="px-4 py-3 text-slate-500">{d.content_type}</td>
                  <td className="px-4 py-3 text-slate-500">{d.chunk_count}</td>
                  <td className="px-4 py-3 text-slate-500">{(d.size_bytes / 1024).toFixed(1)} KB</td>
                  <td className="px-4 py-3">
                    <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${d.status === 'ready' ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-amber-50 text-amber-700 ring-1 ring-amber-600/20'}`}>
                      {d.status}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-slate-500">{new Date(d.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-3 text-right">
                    <button onClick={() => remove(d.id)} className="text-red-600 hover:text-red-500 text-sm font-medium transition-colors">Delete</button>
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
