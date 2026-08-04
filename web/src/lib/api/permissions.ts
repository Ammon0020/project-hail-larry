/**
 * Permissions, pairing, and device management endpoints.
 */

import { apiFetch } from './client'

export interface PermissionOptionInfo {
  id: string
  name: string
  kind: string
}

export interface PendingPermission {
  id: string
  sessionId: string
  tool: string
  command?: string
  target?: string
  options: string[]
  optionDetails?: PermissionOptionInfo[]
}

export interface DeviceCredential {
  id: string
  name: string
  secret: string
  pairedAt: string
}

export interface PairingSession {
  id: string
  token: string
  passcode: string
  url: string
  qrPath: string
  createdAt: string
  expiresAt: string
  used: boolean
}

// Pairing
export function initiatePairing(host: string, port: number) {
  return apiFetch<PairingSession>('/pair/initiate', {
    method: 'POST',
    body: JSON.stringify({ host, port }),
  })
}

export function verifyPasscode(passcode: string, deviceName: string) {
  return apiFetch<DeviceCredential>('/pair/verify-passcode', {
    method: 'POST',
    body: JSON.stringify({ passcode, deviceName }),
  })
}

export function verifyToken(token: string, deviceName: string) {
  return apiFetch<DeviceCredential>('/pair/verify-token', {
    method: 'POST',
    body: JSON.stringify({ token, deviceName }),
  })
}

// Devices
export function listDevices() {
  return apiFetch<DeviceCredential[]>('/devices')
}

export function revokeDevice(deviceId: string) {
  return apiFetch<{ status: string }>(`/devices/${deviceId}`, {
    method: 'DELETE',
  })
}

// Permissions
export function getPendingPermissions() {
  return apiFetch<PendingPermission[]>('/permissions/pending')
}

export function respondPermission(requestId: string, decision: string) {
  return apiFetch<{ status: string }>(`/permissions/${requestId}/respond`, {
    method: 'POST',
    body: JSON.stringify({ decision }),
  })
}
