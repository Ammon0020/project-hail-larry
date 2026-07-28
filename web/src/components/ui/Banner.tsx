import type { ComponentProps } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

const bannerVariants = cva('text-xs', {
  variants: {
    variant: {
      info: 'bg-muted text-muted-foreground border-border',
      warning: 'bg-warning/10 text-warning border-warning/40',
      error: 'bg-destructive/10 text-destructive border-destructive/40',
      success: 'bg-primary/10 text-primary border-primary/40',
    },
  },
})

interface BannerProps
  extends ComponentProps<'div'>,
    VariantProps<typeof bannerVariants> {
  variant: 'info' | 'warning' | 'error' | 'success'
}

/**
 * Shared semantic status banner with visual variants and call-site-controlled
 * layout, spacing, borders, and actions.
 */
export function Banner({ variant, className, ...props }: BannerProps) {
  // error/warning are assertive; info/success are polite status regions.
  const role = variant === 'error' || variant === 'warning' ? 'alert' : 'status'
  const ariaLive = role === 'alert' ? 'assertive' : 'polite'
  return (
    <div
      role={role}
      aria-live={ariaLive}
      aria-atomic="true"
      className={cn(bannerVariants({ variant }), className)}
      {...props}
    />
  )
}
