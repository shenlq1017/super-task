/**
 * Typed client for `/admin/api/*`. Session lives in sessionStorage on purpose:
 * an admin token must not survive a browser restart or leak into other tabs.
 */

const STORAGE_KEY = "st:cloud-admin-session";
export const API_BASE = "/admin/api";

export type Role = "admin" | "user";

export type Session = {
  accountId: string;
  email: string;
  accessToken: string;
  refreshToken: string;
};

export type AdminStatus = {
  admin_available: boolean;
  console_ready: boolean;
};

export type AccountRow = {
  id: string;
  email: string;
  role: Role;
  disabled: boolean;
  /** Epoch seconds — the server's `now()` helper stores `as_secs`. */
  created_at: number;
  entity_count: number;
  entity_bytes: number;
};

type LoginPayload = {
  account_id: string;
  email: string;
  access_token: string;
  refresh_token: string;
  expires_in_secs: number;
};

/** Server error envelope: `{ error, code, message }`. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

function readSession(): Session | null {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Session>;
    if (!parsed.accessToken || !parsed.refreshToken) return null;
    return {
      accountId: parsed.accountId ?? "",
      email: parsed.email ?? "",
      accessToken: parsed.accessToken,
      refreshToken: parsed.refreshToken,
    };
  } catch {
    return null;
  }
}

let session: Session | null = readSession();

export function getSession(): Session | null {
  return session;
}

export function isAuthed(): boolean {
  return session !== null;
}

function persist(next: Session | null) {
  session = next;
  try {
    if (next) sessionStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    else sessionStorage.removeItem(STORAGE_KEY);
  } catch {
    // Private-mode storage failures must not break the current page.
  }
}

export function clearSession() {
  persist(null);
}

function toLoginPayload(body: unknown): LoginPayload {
  const payload = body as Partial<LoginPayload> | null;
  if (!payload?.access_token || !payload.refresh_token) {
    throw new ApiError(0, "BAD_RESPONSE", "登录响应缺少令牌");
  }
  return {
    account_id: payload.account_id ?? "",
    email: payload.email ?? "",
    access_token: payload.access_token,
    refresh_token: payload.refresh_token,
    expires_in_secs: payload.expires_in_secs ?? 0,
  };
}

type RequestOptions = {
  method?: string;
  body?: unknown;
  /** Set false for the endpoints that carry no bearer token. */
  auth?: boolean;
  signal?: AbortSignal;
};

async function parseError(response: Response): Promise<ApiError> {
  let code = `HTTP_${response.status}`;
  let message = response.statusText || "请求失败";
  try {
    const body = (await response.json()) as { code?: string; message?: string };
    if (body.code) code = body.code;
    if (body.message) message = body.message;
  } catch {
    // Non-JSON body (asset handler 404, proxy error page): keep the status-based code.
  }
  return new ApiError(response.status, code, message);
}

async function send(path: string, options: RequestOptions, token: string | null): Promise<unknown> {
  const headers: Record<string, string> = {};
  if (options.body !== undefined) headers["content-type"] = "application/json";
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(`${API_BASE}${path}`, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
    signal: options.signal,
  });
  if (!response.ok) throw await parseError(response);
  if (response.status === 204) return null;
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

// Collapse parallel 401s into a single refresh round-trip.
let refreshInFlight: Promise<Session> | null = null;

async function renewSession(): Promise<Session> {
  if (!session) throw new ApiError(401, "CLOUD_AUTH_FAILED", "会话已失效");
  refreshInFlight ??= (async () => {
    const current = session as Session;
    const payload = toLoginPayload(
      await send("/refresh", { method: "POST", body: { refresh_token: current.refreshToken } }, null),
    );
    const next: Session = {
      accountId: payload.account_id,
      email: payload.email,
      accessToken: payload.access_token,
      refreshToken: payload.refresh_token,
    };
    persist(next);
    return next;
  })().finally(() => {
    refreshInFlight = null;
  });
  return refreshInFlight;
}

/** One refresh-and-replay at most, so an expired session can never loop. */
export async function api<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const useAuth = options.auth !== false;
  const token = useAuth ? session?.accessToken ?? null : null;
  try {
    return (await send(path, options, token)) as T;
  } catch (error) {
    const expired = error instanceof ApiError && error.status === 401 && useAuth && session;
    if (!expired) throw error;
    try {
      const renewed = await renewSession();
      return (await send(path, options, renewed.accessToken)) as T;
    } catch (renewError) {
      clearSession();
      throw renewError;
    }
  }
}

export function status(): Promise<AdminStatus> {
  return api<AdminStatus>("/status", { auth: false });
}

export async function login(email: string, password: string): Promise<Session> {
  const payload = toLoginPayload(await api("/login", { method: "POST", body: { email, password }, auth: false }));
  const next: Session = {
    accountId: payload.account_id,
    email: payload.email,
    accessToken: payload.access_token,
    refreshToken: payload.refresh_token,
  };
  persist(next);
  return next;
}

export function me(): Promise<{ account_id: string; email: string; role: Role }> {
  return api("/me");
}

export function listAccounts(params: { query?: string; limit?: number; offset?: number } = {}): Promise<AccountRow[]> {
  const search = new URLSearchParams();
  if (params.query) search.set("query", params.query);
  if (params.limit !== undefined) search.set("limit", String(params.limit));
  if (params.offset !== undefined) search.set("offset", String(params.offset));
  const suffix = search.toString();
  return api<AccountRow[]>(`/accounts${suffix ? `?${suffix}` : ""}`);
}

export function createAccount(input: { email: string; password: string; role?: Role }): Promise<AccountRow> {
  return api<AccountRow>("/accounts", { method: "POST", body: input });
}

export function setRole(id: string, role: Role): Promise<AccountRow> {
  return api<AccountRow>(`/accounts/${encodeURIComponent(id)}/role`, { method: "PUT", body: { role } });
}

export function setDisabled(id: string, disabled: boolean): Promise<AccountRow> {
  return api<AccountRow>(`/accounts/${encodeURIComponent(id)}/disabled`, { method: "PUT", body: { disabled } });
}

export function setPassword(id: string, password: string): Promise<null> {
  return api<null>(`/accounts/${encodeURIComponent(id)}/password`, { method: "PUT", body: { password } });
}

export function deleteAccount(id: string): Promise<null> {
  return api<null>(`/accounts/${encodeURIComponent(id)}`, { method: "DELETE" });
}

/** Logout is local-only: the server has no revocation endpoint in this scope. */
export function logout() {
  clearSession();
}
