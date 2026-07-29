import { useState } from 'react'
import { AlertTriangle, Check, Copy } from 'lucide-react'
import { cn } from '@/lib/utils'

export function ErrorNote({ message, mono, className }: { message: string; mono?: boolean; className?: string }) {
  return (
    <div className={cn('flex items-start gap-2 p-2 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md', className)}>
      <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
      <span className={mono ? 'font-mono whitespace-pre-wrap break-all' : undefined}>{message}</span>
    </div>
  )
}

export function LabeledInput({
  id, label, value, onChange, placeholder, type = 'text', min, max,
  wrapperClass = 'block', labelClass = 'block text-xs text-muted-foreground mb-1', inputClass = 'w-full',
}: {
  id?: string
  label: string
  value: string | number
  onChange: (value: string) => void
  placeholder?: string
  type?: string
  min?: number
  max?: number
  wrapperClass?: string
  labelClass?: string
  inputClass?: string
}) {
  return (
    <label className={wrapperClass}>
      <span className={labelClass}>{label}</span>
      <input id={id} type={type} min={min} max={max} value={value}
        onChange={event => onChange(event.target.value)} placeholder={placeholder}
        className={cn('bg-background border border-input rounded-md px-3 py-1.5 text-sm', inputClass)} />
    </label>
  )
}

export function CopyableExample({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false)
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // ignore clipboard errors
    }
  }
  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs font-mono text-muted-foreground">{label}</span>
        <button onClick={handleCopy}
          className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted-foreground hover:text-foreground bg-secondary hover:bg-accent rounded border border-border transition">
          {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
          {copied ? 'Copied!' : 'Copy'}
        </button>
      </div>
      <pre className="text-[11px] font-mono bg-muted p-2 rounded border border-border overflow-x-auto text-foreground">{text}</pre>
    </div>
  )
}
