import { useAuthStore } from './auth'

export interface StreamHandlers {
  onThinking?: (delta: string) => void
  onContentDelta?: (delta: string) => void
  onToolCallStart?: (id: string, name: string, input: Record<string, unknown>) => void
  onToolResult?: (id: string, name: string, content: string, isError: boolean) => void
  onDone?: (data: { conversation_id: string; input_tokens: number; output_tokens: number }) => void
  onError?: (message: string) => void
}

/**
 * POST-based SSE client for streaming agent runs.
 * Uses fetch + ReadableStream since native EventSource only supports GET.
 */
export async function fetchSSE(
  url: string,
  body: Record<string, unknown>,
  handlers: StreamHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const token = useAuthStore.getState().accessToken

  const response = await fetch(url, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify(body),
    signal,
  })

  if (!response.ok) {
    const text = await response.text()
    handlers.onError?.(text || `HTTP ${response.status}`)
    return
  }

  const reader = response.body?.getReader()
  if (!reader) {
    handlers.onError?.('No response body')
    return
  }

  const decoder = new TextDecoder()
  let buffer = ''

  while (true) {
    const { done, value } = await reader.read()
    if (done) break

    buffer += decoder.decode(value, { stream: true })

    // Process complete SSE events (double newline separated)
    let idx: number
    while ((idx = buffer.indexOf('\n\n')) !== -1) {
      const block = buffer.slice(0, idx)
      buffer = buffer.slice(idx + 2)

      let eventType = ''
      const dataLines: string[] = []

      for (const line of block.split('\n')) {
        if (line.startsWith('event: ')) eventType = line.slice(7).trim()
        else if (line.startsWith('data: ')) dataLines.push(line.slice(6))
        else if (line.startsWith(':')) continue // comment / keep-alive
      }

      const data = dataLines.join('\n')

      if (!data) continue

      try {
        const parsed = JSON.parse(data)
        switch (eventType) {
          case 'thinking':
            handlers.onThinking?.(parsed.delta)
            break
          case 'content_delta':
            handlers.onContentDelta?.(parsed.delta)
            break
          case 'tool_call_start':
            handlers.onToolCallStart?.(parsed.id, parsed.name, parsed.input)
            break
          case 'tool_result':
            handlers.onToolResult?.(parsed.id, parsed.name, parsed.content, parsed.is_error)
            break
          case 'done':
            handlers.onDone?.(parsed)
            break
          case 'error':
            handlers.onError?.(parsed.message)
            break
        }
      } catch {
        // Skip malformed JSON
      }
    }
  }
}
