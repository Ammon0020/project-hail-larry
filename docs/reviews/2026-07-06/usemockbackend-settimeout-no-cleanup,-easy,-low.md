# useMockBackend.sendPrompt schedules setTimeouts with no cleanup on unmount

- **Difficulty:** easy
- **Urgency:** low
- **File:** `web/src/hooks/useMockBackend.ts`
- **Lines:** 37-61

## Description

The rewritten `sendPrompt` schedules two nested `setTimeout` calls (500ms then 800ms) that call `setEvents`. Neither timer is tracked or cleared on unmount. If the component unmounts between the user message and the simulated agent response, `setEvents` fires on an unmounted component. React 19 no longer warns about this, but it is still a state update after unmount and a minor leak. The diff rewrote this entire block (adding the `attachments` param and reformatting), so it was a natural point to add cleanup.

## Recommendation

Store the timer ids in a `useRef<number[]>` and clear them in a `useEffect` cleanup, or use a `mountedRef` guard inside the timeouts. Since this is a mock, the simplest fix is a `mountedRef` checked before each `setEvents`.

## Verification

`read` of useMockBackend.ts:22-64 confirms no `useEffect` cleanup and no timer refs; the hook has no unmount handling at all.
