export async function withAsyncState<T>(
  setLoading: (loading: boolean) => void,
  setError: (error: string | null) => void,
  fn: () => Promise<T>,
): Promise<T | undefined> {
  setLoading(true)
  setError(null)
  try {
    return await fn()
  } catch (error) {
    setError(error instanceof Error ? error.message : String(error))
  } finally {
    setLoading(false)
  }
}
