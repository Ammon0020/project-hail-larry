# Device pairing credential stored in localStorage — XSS-accessible

- **Difficulty:** easy
- **Urgency:** high
- **File:** `web/src/lib/api.ts`
- **Lines:** 14-34, 24-34

## Description

The paired device credential (`{ id, secret }`) is persisted in `localStorage` under `lai:deviceCredential` and read on every API call (`getDeviceCredential` → `authHeader`). `localStorage` is fully readable by any JavaScript running in the IDE origin. The credential grants full authenticated API access (file read/write, shell command approval, device management). Any XSS — however unlikely today — would silently exfiltrate the long-lived device secret, giving the attacker persistent access until the device is revoked. The secret is also written from `LockScreen.tsx:37` and `useBackend.ts:643`. There is no expiry enforced client-side and no rotation.

## Recommendation

Prefer an `HttpOnly; SameSite=Strict; Secure` cookie set by the backend on `POST /api/pair/verify-passcode` for the primary credential, with the `Authorization` header approach retained only as a fallback for non-browser clients. If localStorage must be used (e.g. for the WS handshake), scope the secret to a short-lived session token that the backend rotates, and add a strict CSP (see spa-no-csp) so the XSS surface that could read it is minimized.

## Verification

`api.ts:26` `localStorage.getItem(DEVICE_CREDENTIAL_KEY)`; `api.ts:47` returns `Bearer ${cred.id}:${cred.secret}`; `useBackend.ts:643` `localStorage.setItem('lai:deviceCredential', JSON.stringify(cred))`; `LockScreen.tsx:37` same. No `sessionStorage`/cookie alternative exists in the codebase.
