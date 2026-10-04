import { useCallback, useEffect, useState } from "react";

export type User = { id: string; username: string; is_admin: boolean };
export type Scope = { repository: string; actions: string[] };
export type Token = {
  id: string;
  name: string;
  prefix: string;
  scopes: Scope[];
  namespace_pull?: string | null;
  expires_at: number | null;
  last_used_at: number | null;
  revoked_at: number | null;
};
export type Descriptor = { media_type: string; digest: string; size: number };
export type Tag = {
  tag: string;
  digest: string;
  media_type: string;
  size: number;
  created_at: number;
  references?: Descriptor[];
  subject?: Descriptor | null;
};
export type Repository = {
  name: string;
  visibility: string;
  tag_count?: number;
  manifest_count?: number;
  latest_tag?: string | null;
  latest_digest?: string | null;
  size?: number;
  updated_at?: number | null;
};
export type RepositoryDetail = {
  name: string;
  visibility: string;
  tags: Tag[];
};
export type AuditEvent = {
  id: string;
  kind: string;
  occurred_at: number;
  actor: string | null;
  repository: string | null;
  tag: string | null;
  digest: string | null;
  metadata: Record<string, unknown>;
};
export type Webhook = { id: string; url: string; events: string[]; enabled: boolean; created_at: number };
export type Health = { status: string; storage: string; database: string };
export type Overview = {
  user: User;
  repository_count: number;
  repositories: Repository[];
  storage_bytes: number;
  referenced_bytes: number;
  unreferenced_bytes: number;
  active_token_count: number;
  events: AuditEvent[];
  health: Health;
  uptime_seconds: number;
};
export type Instance = {
  public_url: string;
  registry_host: string;
  environment: string;
  storage_backend: string;
  storage_bucket: string | null;
  pull_mode: string;
  token_service: string;
  token_ttl_seconds: number;
  registration: string;
};
export type StorageOverview = {
  repository_count: number;
  total_bytes: number;
  referenced_bytes: number;
  unreferenced_bytes: number;
  repositories: Repository[];
};
export type UploadSession = {
  id: string;
  repository: string;
  staging_key: string;
  offset: number;
  expires_at: number;
  status: string;
};
export type GcReport = {
  dry_run: boolean;
  manifests: number;
  blobs: number;
  reclaimed_bytes: number;
  failures: string[];
};

export type ApiError = Error & { status?: number; code?: string };

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: "include",
    ...init,
    headers: { ...(init.body ? { "Content-Type": "application/json" } : {}), ...(init.headers ?? {}) }
  });
  if (response.status === 204) return undefined as T;
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(typeof body.error === "string" ? body.error : "Request failed") as ApiError;
    error.status = response.status;
    error.code = body.error;
    throw error;
  }
  return body as T;
}

export function friendlyError(error: unknown): string {
  const apiError = error as ApiError;
  if (apiError.code === "two_factor_required") return "Enter the code from your authenticator app to continue.";
  if (apiError.code === "two_factor_invalid") return "The authenticator code is invalid or expired.";
  if (apiError.code === "weak_password") return "Use a password with at least 12 characters.";
  if (apiError.code === "password_mismatch") return "The new passwords do not match.";
  if (apiError.code === "two_factor_already_enabled") return "Two-factor authentication is already enabled.";
  if (apiError.code === "two_factor_not_enabled") return "Two-factor authentication is not enabled.";
  if (apiError.code === "two_factor_setup_missing") return "Start two-factor setup before confirming a code.";
  if (apiError.status === 401) return "Your session expired. Sign in again to continue.";
  if (apiError.status === 403) return "You don't have permission to access this page.";
  if (apiError.status === 404) return "The requested resource was not found.";
  if (apiError.status === 409) return "This change conflicts with the current registry state.";
  if (apiError.status === 429) return "Too many requests. Wait a moment and retry.";
  if (apiError.message === "unauthorized") return "The username or password is incorrect.";
  if (apiError.message === "forbidden") return "You don't have permission to perform this action.";
  if (apiError.message === "Failed to fetch" || apiError.name === "TypeError") {
    return "The registry API did not respond.";
  }
  return apiError.message || "Something went wrong.";
}

/** Fetch a JSON resource on mount and whenever `path` changes. */
export function useResource<T>(path: string | null) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState("");
  const [version, setVersion] = useState(0);
  useEffect(() => {
    if (!path) return;
    let live = true;
    setError("");
    api<T>(path)
      .then((value) => live && setData(value))
      .catch((reason) => live && setError(friendlyError(reason)));
    return () => {
      live = false;
    };
  }, [path, version]);
  const reload = useCallback(() => setVersion((value) => value + 1), []);
  return { data, error, reload, setData };
}
