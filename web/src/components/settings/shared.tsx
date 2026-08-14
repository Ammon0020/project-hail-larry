import { useState } from 'react'
import { AlertTriangle, Check, Copy } from 'lucide-react'
import { cn } from '@/lib/utils'

export function ErrorNote({ message, mono, className }: { message: string; mono?: boolean; className?: string }) {
  return (
    <div className={cn('flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-2 text-xs text-destructive', className)}>
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
      <span className={mono ? 'font-mono break-all whitespace-pre-wrap' : undefined}>{message}</span>
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
        className={cn('rounded-md border border-input bg-background px-3 py-1.5 text-sm', inputClass)} />
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
      <div className="mb-1 flex items-center justify-between">
        <span className="font-mono text-xs text-muted-foreground">{label}</span>
        <button onClick={handleCopy}
          className="flex items-center gap-1 rounded border border-border bg-secondary px-2 py-0.5 text-[10px] text-muted-foreground transition hover:bg-accent hover:text-foreground">
          {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
          {copied ? 'Copied!' : 'Copy'}
        </button>
      </div>
      <pre className="overflow-x-auto rounded border border-border bg-muted p-2 font-mono text-[11px] text-foreground">{text}</pre>
    </div>
  )
}
