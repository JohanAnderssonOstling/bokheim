import type { AccountStatus } from "../bindings";
import { getWebDatabase } from "./database.ts";
import { decodeAccountResponse, encodeAccountRequest } from "./wasm-core.ts";

const API_PREFIX = "/api/auth";
const SERVER_URL = "https://api.bokheim.se";
export const WIRE_MEDIA_TYPE = "application/vnd.bokheim+protobuf; version=13";

const requestKind = {
  login: 1,
  registration: 2,
  email: 3,
  verification: 4,
  resetPassword: 5,
} as const;

interface WebAccountProfile {
  user_id: string;
  email: string;
}

export interface StoredWebAccountSession extends WebAccountProfile {
  server_url: string;
}

function signedOut(): AccountStatus {
  return { signed_in: false, server_url: null, email: null };
}

function accountStatus(profile: StoredWebAccountSession): AccountStatus {
  return { signed_in: true, server_url: profile.server_url, email: profile.email };
}

async function readProfile(): Promise<StoredWebAccountSession | null> {
  const { db } = await getWebDatabase();
  const row = db.exec({
    sql: "SELECT server_url, user_id, email FROM web_account_profile WHERE singleton=1",
    rowMode: "object", returnValue: "resultRows",
  })[0] as Record<string, string> | undefined;
  return row ? { server_url: row.server_url, user_id: row.user_id, email: row.email } : null;
}

async function writeProfile(profile: WebAccountProfile): Promise<StoredWebAccountSession> {
  const stored = { ...profile, server_url: SERVER_URL };
  const { db } = await getWebDatabase();
  db.exec({
    sql: `INSERT INTO web_account_profile (singleton, server_url, user_id, email) VALUES (1, ?, ?, ?)
          ON CONFLICT (singleton) DO UPDATE SET
            server_url=excluded.server_url, user_id=excluded.user_id, email=excluded.email`,
    bind: [stored.server_url, stored.user_id, stored.email],
  });
  return stored;
}

async function clearProfile(): Promise<void> {
  const { db } = await getWebDatabase();
  db.exec("DELETE FROM web_account_profile WHERE singleton=1");
}

async function checked(response: Response, context: string): Promise<Response> {
  if (response.ok) return response;
  const message = (await response.text()).trim();
  throw new Error(message || `${context} returned ${response.status}`);
}

async function post(path: string, kind: number, value: unknown): Promise<Response> {
  const body = await encodeAccountRequest(kind, value);
  return checked(await fetch(`${API_PREFIX}/${path}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { Accept: WIRE_MEDIA_TYPE, "Content-Type": WIRE_MEDIA_TYPE },
    body,
  }), "Account server");
}

async function profileResponse(response: Response): Promise<StoredWebAccountSession> {
  const profile = await decodeAccountResponse<WebAccountProfile>(2, new Uint8Array(await response.arrayBuffer()));
  return writeProfile(profile);
}

async function performWebSessionRefresh(): Promise<StoredWebAccountSession> {
  const response = await checked(await fetch(`${API_PREFIX}/web/refresh`, {
    method: "POST", credentials: "same-origin", headers: { Accept: WIRE_MEDIA_TYPE },
  }), "Session refresh");
  return profileResponse(response);
}

let refreshPromise: Promise<StoredWebAccountSession> | undefined;

async function refreshWebSession(): Promise<StoredWebAccountSession> {
  if (!refreshPromise) {
    refreshPromise = performWebSessionRefresh().finally(() => (refreshPromise = undefined));
  }
  return refreshPromise;
}

async function recoverCookieProfile(): Promise<StoredWebAccountSession | null> {
  const request = () => fetch(`${API_PREFIX}/web/me`, { credentials: "same-origin", headers: { Accept: WIRE_MEDIA_TYPE } });
  try {
    let response = await request();
    if (response.status === 401) {
      await response.arrayBuffer();
      await refreshWebSession();
      response = await request();
    }
    if (response.status === 401) return null;
    return profileResponse(await checked(response, "Account session"));
  } catch {
    return null;
  }
}

export async function authenticatedWebFetch(path: string, init: RequestInit = {}, allowedStatuses: readonly number[] = []): Promise<Response> {
  if (!await readProfile() && !await recoverCookieProfile()) throw new Error("Sign in to synchronize this library");
  const request = () => {
    const headers = new Headers(init.headers);
    if (!headers.has("Accept")) headers.set("Accept", WIRE_MEDIA_TYPE);
    return fetch(path, { ...init, credentials: "same-origin", headers });
  };
  let response: Response | undefined;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      response = await request();
      if (response.status === 401) {
        await response.arrayBuffer();
        await refreshWebSession();
        response = await request();
      }
      if (![429, 502, 503, 504].includes(response.status) || attempt === 2) break;
      await response.arrayBuffer();
    } catch (reason) {
      if (!(reason instanceof TypeError) || attempt === 2) throw reason;
    }
    await new Promise((resolve) => setTimeout(resolve, 250 * 2 ** attempt));
  }
  if (!response) throw new Error("Synchronization request did not complete");
  return allowedStatuses.includes(response.status) ? response : checked(response, "Synchronization server");
}

export async function getWebAccountSession(): Promise<StoredWebAccountSession | null> {
  return await readProfile() ?? recoverCookieProfile();
}

export async function getWebAccountStatus(): Promise<AccountStatus> {
  const profile = await getWebAccountSession();
  return profile ? accountStatus(profile) : signedOut();
}

export async function loginWebAccount(email: string, password: string): Promise<AccountStatus> {
  return accountStatus(await profileResponse(await post("web/login", requestKind.login, { email, password })));
}

export async function registerWebAccount(email: string, password: string): Promise<void> {
  await post("register", requestKind.registration, { email, password });
}

export async function resendWebVerification(email: string): Promise<void> {
  await post("resend-verification", requestKind.email, { email });
}

export async function verifyWebEmail(email: string, pin: string): Promise<AccountStatus> {
  return accountStatus(await profileResponse(await post("web/verify-email", requestKind.verification, { email, pin })));
}

export async function requestWebPasswordReset(email: string): Promise<void> {
  await post("request-password-reset", requestKind.email, { email });
}

export async function resetWebPassword(token: string, newPassword: string): Promise<void> {
  await post("reset-password", requestKind.resetPassword, { token, new_password: newPassword });
}

export async function logoutWebAccount(): Promise<AccountStatus> {
  try {
    await fetch(`${API_PREFIX}/web/logout`, { method: "POST", credentials: "same-origin", headers: { Accept: WIRE_MEDIA_TYPE } });
  } finally {
    await clearProfile();
  }
  return signedOut();
}

export async function getWebAccountStorageUsage(): Promise<{ used_bytes: number; reserved_bytes: number; quota_bytes: number }> {
  const response = await authenticatedWebFetch("/api/account/storage");
  return decodeAccountResponse(3, new Uint8Array(await response.arrayBuffer()));
}

export async function getWebServerLibraryStorageUsage(): Promise<Array<{ library_id: string; used_bytes: number }>> {
  const response = await authenticatedWebFetch("/api/account/storage/libraries");
  const value = await decodeAccountResponse<{ libraries: Array<{ library_id: string; used_bytes: number }> }>(9, new Uint8Array(await response.arrayBuffer()));
  return value.libraries;
}
