export interface ToolCallBlock {
  id?: string
  name: string
  input: Record<string, unknown>
  result?: string
  is_error?: boolean
}

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
  thinking?: string
  toolCalls?: ToolCallBlock[]
}

export interface ChatPanelProps {
  messages: ChatMessage[]
  loading: boolean
  onSend: (text: string) => void
  onStop?: () => void
  disabled?: boolean
  placeholder?: string
  emptyStateText?: string
  emptyStateSubtext?: string
}
