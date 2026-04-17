import { useState, useEffect, useCallback } from 'react'
import api from '../lib/api'
import type { Deployment, AvailableModel, AvailableBackend } from '../types'

// ── Form state ──────────────────────────────────────────────────

interface DeploymentForm {
  name: string
  slug: string
  model_id: string
  fallback_deployment_id: string
  retry_attempts: number
  retry_backoff_ms: number
  fallback_enabled: boolean
  timeout_ms: number
  rate_limit_rpm: string
  temperature: string
  max_tokens: string
  top_p: string
  extra_params: string
}

const emptyForm: DeploymentForm = {
  name: '',
  slug: '',
  model_id: '',
  fallback_deployment_id: '',
  retry_attempts: 1,
  retry_backoff_ms: 1000,
  fallback_enabled: true,
  timeout_ms: 30000,
  rate_limit_rpm: '',
  temperature: '',
  max_tokens: '',
  top_p: '',
  extra_params: '{}',
}

function formFromDeployment(d: Deployment): DeploymentForm {
  const dp = d.default_params ?? {}
  return {
    name: d.name,
    slug: d.slug,
    model_id: d.model_id,
    fallback_deployment_id: d.fallback_deployment_id ?? '',
    retry_attempts: d.retry_attempts,
    retry_backoff_ms: d.retry_backoff_ms,
    fallback_enabled: d.fallback_enabled,
    timeout_ms: d.timeout_ms,
    rate_limit_rpm: d.rate_limit_rpm != null ? String(d.rate_limit_rpm) : '',
    temperature: dp.temperature != null ? String(dp.temperature) : '',
    max_tokens: dp.max_tokens != null ? String(dp.max_tokens) : '',
    top_p: dp.top_p != null ? String(dp.top_p) : '',
    extra_params: (() => {
      const { temperature: _t, max_tokens: _m, top_p: _p, ...rest } = dp as Record<string, unknown>
      return Object.keys(rest).length > 0 ? JSON.stringify(rest, null, 2) : '{}'
    })(),
  }
}

function formToPayload(form: DeploymentForm) {
  const default_params: Record<string, unknown> = {}
  if (form.temperature !== '') default_params.temperature = parseFloat(form.temperature)
  if (form.max_tokens !== '') default_params.max_tokens = parseInt(form.max_tokens, 10)
  if (form.top_p !== '') default_params.top_p = parseFloat(form.top_p)
  try {
    const extra = JSON.parse(form.extra_params)
    if (typeof extra === 'object' && extra !== null) Object.assign(default_params, extra)
  } catch { /* ignore */ }

  return {
    name: form.name,
    slug: form.slug,
    model_id: form.model_id,
    fallback_deployment_id: form.fallback_deployment_id || null,
    retry_attempts: form.retry_attempts,
    retry_backoff_ms: form.retry_backoff_ms,
    fallback_enabled: form.fallback_enabled,
    timeout_ms: form.timeout_ms,
    default_params,
    rate_limit_rpm: form.rate_limit_rpm ? parseInt(form.rate_limit_rpm, 10) : null,
  }
}

// ── Styles ──────────────────────────────────────────────────────

const inputCls = 'w-full rounded-lg ring-1 ring-slate-300 px-3 py-2 text-sm shadow-sm focus:ring-2 focus:ring-blue-600 focus:outline-none transition-shadow'
const selectCls = inputCls + ' bg-white'
const labelCls = 'block text-xs font-medium text-slate-600 mb-1'
const sectionCls = 'border-t border-slate-100 pt-4 mt-4'

// ── Component ───────────────────────────────────────────────────

export default function DeploymentListPage() {
  const [deployments, setDeployments] = useState<Deployment[]>([])
  const [models, setModels] = useState<AvailableModel[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  // null = closed, 'new' = create, string = edit id
  const [editingId, setEditingId] = useState<string | null>(null)
  const [form, setForm] = useState<DeploymentForm>(emptyForm)

  const load = useCallback(() => {
    setLoading(true)
    Promise.all([
      api.get('/api/v1/deployments?per_page=100'),
      api.get('/api/v1/deployments/available-models'),
    ])
      .then(([depRes, modRes]) => {
        setDeployments(depRes.data.data ?? depRes.data)
        setModels(modRes.data)
      })
      .catch((e) => setError(e.response?.data?.message ?? e.message))
      .finally(() => setLoading(false))
  }, [])

  useEffect(load, [load])

  const openCreate = () => {
    setForm(emptyForm)
    setEditingId('new')
    setError('')
  }

  const openEdit = (d: Deployment) => {
    setForm(formFromDeployment(d))
    setEditingId(d.id)
    setError('')
  }

  const close = () => {
    setEditingId(null)
    setError('')
  }

  const save = async () => {
    setSaving(true)
    setError('')
    try {
      const payload = formToPayload(form)
      if (editingId === 'new') {
        await api.post('/api/v1/deployments', payload)
      } else {
        await api.patch(`/api/v1/deployments/${editingId}`, payload)
      }
      close()
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: string) => {
    if (!confirm('Delete this deployment?')) return
    try {
      await api.delete(`/api/v1/deployments/${id}`)
      load()
    } catch (e: any) {
      setError(e.response?.data?.message ?? e.message)
    }
  }

  const autoSlug = (name: string) =>
    name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  const selectedModel = models.find((m) => m.id === form.model_id)

  // Other deployments available as fallback targets (exclude self)
  const fallbackOptions = deployments.filter((d) => d.id !== editingId && d.is_active)

  // Build fallback chain for display
  const getFallbackChain = (d: Deployment): string[] => {
    const chain: string[] = []
    let current: Deployment | undefined = d
    const seen = new Set<string>()
    while (current?.fallback_deployment_id) {
      if (seen.has(current.fallback_deployment_id)) break
      seen.add(current.fallback_deployment_id)
      const next = deployments.find((x) => x.id === current!.fallback_deployment_id)
      if (!next) break
      chain.push(next.name)
      current = next
    }
    return chain
  }

  if (loading) return <p className="text-slate-500">Loading...</p>

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h1 className="font-display text-3xl tracking-tight text-slate-900">Deployments</h1>
        {editingId === null && (
          <button onClick={openCreate} className="bg-blue-600 text-white px-4 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors">
            Create
          </button>
        )}
      </div>

      {error && <div className="bg-red-50 text-red-700 p-3 rounded-xl ring-1 ring-red-200 text-sm mb-4">{error}</div>}

      {/* ── Form (create / edit) ── */}
      {editingId !== null && (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-5 mb-6">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold text-slate-900">
              {editingId === 'new' ? 'New Deployment' : 'Edit Deployment'}
            </h2>
            <button onClick={close} className="text-slate-400 hover:text-slate-600 text-sm">Cancel</button>
          </div>

          {/* Basic info */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label className={labelCls}>Name</label>
              <input value={form.name} onChange={(e) => {
                const name = e.target.value
                setForm((f) => ({
                  ...f,
                  name,
                  slug: editingId === 'new' ? autoSlug(name) : f.slug,
                }))
              }} placeholder="e.g. Sonnet Fast" className={inputCls} />
            </div>
            <div>
              <label className={labelCls}>Slug</label>
              <input value={form.slug} onChange={(e) => setForm({ ...form, slug: e.target.value })}
                placeholder="e.g. sonnet-fast" className={inputCls + ' font-mono'} />
              <p className="text-xs text-slate-400 mt-1">Used in API calls as the model name</p>
            </div>
          </div>

          {/* Model selection */}
          <div className={sectionCls}>
            <label className={labelCls}>Model</label>
            {models.length === 0 ? (
              <p className="text-sm text-slate-500">No models available. Ensure models are published and quotas are assigned.</p>
            ) : (
              <select value={form.model_id} onChange={(e) => setForm({ ...form, model_id: e.target.value })}
                className={selectCls}>
                <option value="">Select a model...</option>
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.display_name} ({m.provider}) {m.context_window ? `\u2014 ${(m.context_window / 1000).toFixed(0)}k ctx` : ''}
                  </option>
                ))}
              </select>
            )}
            {selectedModel && (
              <div className="flex gap-2 mt-2 flex-wrap">
                {selectedModel.cap_chat && <span className="text-xs bg-blue-50 text-blue-700 px-2 py-0.5 rounded-full ring-1 ring-blue-600/20">Chat</span>}
                {selectedModel.cap_vision && <span className="text-xs bg-purple-50 text-purple-700 px-2 py-0.5 rounded-full ring-1 ring-purple-600/20">Vision</span>}
                {selectedModel.cap_tool_use && <span className="text-xs bg-green-50 text-green-700 px-2 py-0.5 rounded-full ring-1 ring-green-600/20">Tools</span>}
                {selectedModel.cap_thinking && <span className="text-xs bg-amber-50 text-amber-700 px-2 py-0.5 rounded-full ring-1 ring-amber-600/20">Thinking</span>}
                {selectedModel.max_output_tokens && <span className="text-xs bg-slate-100 text-slate-600 px-2 py-0.5 rounded-full">{(selectedModel.max_output_tokens / 1000).toFixed(0)}k max output</span>}
              </div>
            )}
          </div>

          {/* Default params */}
          <div className={sectionCls}>
            <h3 className="text-sm font-semibold text-slate-700 mb-3">Default Parameters</h3>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div>
                <label className={labelCls}>Temperature</label>
                <input type="number" step="0.1" min="0" max="2" value={form.temperature}
                  onChange={(e) => setForm({ ...form, temperature: e.target.value })}
                  placeholder="Provider default" className={inputCls} />
              </div>
              <div>
                <label className={labelCls}>Max Tokens</label>
                <input type="number" step="1" min="1" value={form.max_tokens}
                  onChange={(e) => setForm({ ...form, max_tokens: e.target.value })}
                  placeholder="Provider default" className={inputCls} />
              </div>
              <div>
                <label className={labelCls}>Top P</label>
                <input type="number" step="0.05" min="0" max="1" value={form.top_p}
                  onChange={(e) => setForm({ ...form, top_p: e.target.value })}
                  placeholder="Provider default" className={inputCls} />
              </div>
            </div>
            <details className="mt-3">
              <summary className="text-xs text-slate-500 cursor-pointer hover:text-slate-700">Extra parameters (JSON)</summary>
              <textarea value={form.extra_params} onChange={(e) => setForm({ ...form, extra_params: e.target.value })}
                className={inputCls + ' font-mono mt-2'} rows={3} placeholder='e.g. {"top_k": 40}' />
            </details>
          </div>

          {/* Fallback deployment */}
          <div className={sectionCls}>
            <h3 className="text-sm font-semibold text-slate-700 mb-1">Fallback Deployment</h3>
            <p className="text-xs text-slate-400 mb-3">
              If all backends for this deployment fail, route the request to a different deployment (different model).
            </p>
            <select value={form.fallback_deployment_id}
              onChange={(e) => setForm({ ...form, fallback_deployment_id: e.target.value })}
              className={selectCls}>
              <option value="">No fallback</option>
              {fallbackOptions.map((d) => {
                const m = models.find((x) => x.id === d.model_id)
                return (
                  <option key={d.id} value={d.id}>
                    {d.name} ({d.slug}) {m ? `\u2014 ${m.display_name}` : ''}
                  </option>
                )
              })}
            </select>
            {form.fallback_deployment_id && (() => {
              const fb = deployments.find((d) => d.id === form.fallback_deployment_id)
              if (!fb) return null
              const chain = getFallbackChain(fb)
              if (chain.length === 0) return null
              return (
                <p className="text-xs text-slate-400 mt-1">
                  Chain: {fb.name} {chain.map((n) => ` \u2192 ${n}`).join('')}
                </p>
              )
            })()}
          </div>

          {/* Retry / Timeout */}
          <div className={sectionCls}>
            <h3 className="text-sm font-semibold text-slate-700 mb-3">Resilience</h3>
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
              <div>
                <label className={labelCls}>Retry Attempts</label>
                <input type="number" min="0" max="5" value={form.retry_attempts}
                  onChange={(e) => setForm({ ...form, retry_attempts: parseInt(e.target.value, 10) || 0 })}
                  className={inputCls} />
              </div>
              <div>
                <label className={labelCls}>Backoff (ms)</label>
                <input type="number" min="100" step="100" value={form.retry_backoff_ms}
                  onChange={(e) => setForm({ ...form, retry_backoff_ms: parseInt(e.target.value, 10) || 1000 })}
                  className={inputCls} />
              </div>
              <div>
                <label className={labelCls}>Timeout (ms)</label>
                <input type="number" min="1000" step="1000" value={form.timeout_ms}
                  onChange={(e) => setForm({ ...form, timeout_ms: parseInt(e.target.value, 10) || 30000 })}
                  className={inputCls} />
              </div>
              <div>
                <label className={labelCls}>Rate Limit (rpm)</label>
                <input type="number" min="0" value={form.rate_limit_rpm}
                  onChange={(e) => setForm({ ...form, rate_limit_rpm: e.target.value })}
                  placeholder="No limit" className={inputCls} />
              </div>
            </div>
          </div>

          {/* Save button */}
          <div className="mt-5 flex gap-3">
            <button onClick={save} disabled={saving || !form.name || !form.slug || !form.model_id}
              className="bg-blue-600 text-white px-5 py-2 rounded-full text-sm font-semibold hover:bg-blue-500 active:bg-blue-800 transition-colors disabled:opacity-50">
              {saving ? 'Saving...' : editingId === 'new' ? 'Create Deployment' : 'Save Changes'}
            </button>
            <button onClick={close} className="text-slate-500 hover:text-slate-700 text-sm px-3">Cancel</button>
          </div>
        </div>
      )}

      {/* ── Deployment list ── */}
      {deployments.length === 0 ? (
        <p className="text-slate-500">No deployments yet.</p>
      ) : (
        <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-left text-slate-600">
              <tr>
                <th className="px-4 py-3">Name</th>
                <th className="px-4 py-3">Slug</th>
                <th className="px-4 py-3">Model</th>
                <th className="px-4 py-3">Fallback</th>
                <th className="px-4 py-3">Timeout</th>
                <th className="px-4 py-3">Status</th>
                <th className="px-4 py-3"></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {deployments.map((d) => {
                const model = models.find((m) => m.id === d.model_id)
                const fallback = d.fallback_deployment_id
                  ? deployments.find((x) => x.id === d.fallback_deployment_id)
                  : null
                return (
                  <tr key={d.id} className="hover:bg-slate-50">
                    <td className="px-4 py-3 font-medium text-slate-900">{d.name}</td>
                    <td className="px-4 py-3 text-slate-500 font-mono text-xs">{d.slug}</td>
                    <td className="px-4 py-3 text-slate-600">
                      {model ? (
                        <span>{model.display_name} <span className="text-slate-400 text-xs">({model.provider})</span></span>
                      ) : (
                        <span className="text-slate-400 font-mono text-xs">{d.model_id.slice(0, 8)}...</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-slate-500 text-xs">
                      {fallback ? (
                        <span className="text-blue-600">{fallback.name}</span>
                      ) : (
                        <span className="text-slate-300">&mdash;</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-slate-500 text-xs">{(d.timeout_ms / 1000).toFixed(0)}s</td>
                    <td className="px-4 py-3">
                      <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${d.is_active ? 'bg-green-50 text-green-700 ring-1 ring-green-600/20' : 'bg-slate-50 text-slate-600 ring-1 ring-slate-600/20'}`}>
                        {d.is_active ? 'Active' : 'Inactive'}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-right space-x-3">
                      <button onClick={() => openEdit(d)} className="text-blue-600 hover:text-blue-500 text-sm font-medium transition-colors">Edit</button>
                      <button onClick={() => remove(d.id)} className="text-red-600 hover:text-red-500 text-sm font-medium transition-colors">Delete</button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
