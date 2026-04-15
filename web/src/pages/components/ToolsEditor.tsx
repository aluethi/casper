// ── Built-in tool config types ───────────────────────────────────
const BUILTIN_TOOLS = [
  { name: 'delegate', label: 'Delegate', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 300 }, { key: 'max_depth', label: 'Max depth', type: 'number', default: 3 }] },
  { name: 'ask_user', label: 'Ask User', fields: [] },
  { name: 'knowledge_search', label: 'Knowledge Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 5 }, { key: 'relevance_threshold', label: 'Threshold', type: 'number', default: 0.7 }] },
  { name: 'update_memory', label: 'Update Memory', fields: [{ key: 'max_document_tokens', label: 'Max tokens', type: 'number', default: 4000 }] },
  { name: 'web_search', label: 'Web Search', fields: [{ key: 'max_results', label: 'Max results', type: 'number', default: 10 }] },
  { name: 'web_fetch', label: 'Web Fetch', fields: [{ key: 'timeout_secs', label: 'Timeout (s)', type: 'number', default: 30 }, { key: 'max_response_bytes', label: 'Max bytes', type: 'number', default: 1048576 }] },
]

interface ToolsEditorProps {
  builtinTools: Record<string, Record<string, unknown>>
  setBuiltinTools: React.Dispatch<React.SetStateAction<Record<string, Record<string, unknown>>>>
}

export default function ToolsEditor({ builtinTools, setBuiltinTools }: ToolsEditorProps) {
  return (
    <div className="bg-white rounded-2xl ring-1 ring-slate-900/5 shadow-sm p-6">
      <h2 className="font-display text-lg tracking-tight text-slate-900 mb-4">Built-in Tools</h2>
      <div className="space-y-3">
        {BUILTIN_TOOLS.map(tool => {
          const enabled = tool.name in builtinTools
          const config = builtinTools[tool.name] || {}
          return (
            <div key={tool.name} className={`rounded-xl ring-1 px-4 py-3 transition-all ${enabled ? 'ring-blue-200 bg-blue-50/30' : 'ring-slate-200'}`}>
              <div className="flex items-center justify-between">
                <label className="flex items-center gap-3 cursor-pointer">
                  <input type="checkbox" checked={enabled} onChange={e => {
                    const next = { ...builtinTools }
                    if (e.target.checked) {
                      const defaults: Record<string, unknown> = {}
                      tool.fields.forEach(f => { defaults[f.key] = f.default })
                      next[tool.name] = defaults
                    } else { delete next[tool.name] }
                    setBuiltinTools(next)
                  }} className="rounded border-slate-300 text-blue-600 focus:ring-blue-500" />
                  <span className="text-sm font-medium text-slate-900">{tool.label}</span>
                </label>
              </div>
              {enabled && tool.fields.length > 0 && (
                <div className="mt-3 flex gap-4 pl-8">
                  {tool.fields.map(f => (
                    <div key={f.key}>
                      <label className="block text-xs text-slate-500 mb-0.5">{f.label}</label>
                      <input type="number" value={(config[f.key] as number) ?? f.default}
                        onChange={e => setBuiltinTools({ ...builtinTools, [tool.name]: { ...config, [f.key]: +e.target.value } })}
                        className="w-28 rounded-lg ring-1 ring-slate-300 px-2 py-1 text-sm" />
                    </div>
                  ))}
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
