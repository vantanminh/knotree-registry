export type User = { id: string; username: string; is_admin: boolean };
export type Token = {
  id: string;
  name: string;
  prefix: string;
  scopes: { repository: string; actions: string[] }[];
  expires_at: number | null;
  last_used_at: number | null;
  revoked_at: number | null;
};
export type Overview = {
  user: User;
  repository_count: number;
  repositories: string[];
  active_token_count: number;
};
export type Repository = { name: string; visibility: string };
export type RepositoryDetail = {
  name: string;
  visibility: string;
  tags: { tag: string; digest: string; media_type: string; size: number; created_at: number }[];
};
export type AuditEvent = { id: string; kind: string; occurred_at: number; actor: string | null; repository: string | null; tag: string | null; digest: string | null; metadata: Record<string, unknown> };
export type Webhook = { id: string; url: string; events: string[]; enabled: boolean; created_at: number };

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: "include",
    ...init,
    headers: { ...(init.body ? { "Content-Type": "application/json" } : {}), ...(init.headers ?? {}) }
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(body.error ?? "Request failed") as Error & { status?: number };
    error.status = response.status;
    throw error;
  }
  return body as T;
}

export function formatDate(epoch: number | null | undefined): string {
  if (!epoch) return "Never";
  return new Date(epoch * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
