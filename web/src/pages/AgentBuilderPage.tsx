import { useParams } from 'react-router-dom'

export default function AgentBuilderPage() {
  const { name } = useParams<{ name: string }>()

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 mb-4">
        Agent: {name}
      </h1>
      <div className="bg-white rounded-lg border border-gray-200 p-6">
        <p className="text-gray-500">Agent builder coming soon.</p>
      </div>
    </div>
  )
}
