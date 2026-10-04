export function formatBytes(value = 0): string {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  const digits = size >= 100 || unit === 0 ? 0 : size >= 10 ? 1 : 2;
  return `${size.toFixed(digits)} ${units[unit]}`;
}

export function formatAbsolute(epoch?: number | null): string {
  if (!epoch) return "Never";
  return new Date(epoch * 1000).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });
}

export function formatRelative(epoch?: number | null, now = Date.now()): string {
  if (!epoch) return "Never";
  const delta = Math.round(now / 1000 - epoch);
  const future = delta < 0;
  const abs = Math.abs(delta);
  const say = (text: string) => (future ? `in ${text}` : `${text} ago`);
  if (abs < 10) return "just now";
  if (abs < 60) return say(`${abs}s`);
  if (abs < 3600) return say(`${Math.floor(abs / 60)}m`);
  if (abs < 86400) return say(`${Math.floor(abs / 3600)}h`);
  if (abs < 30 * 86400) return say(`${Math.floor(abs / 86400)}d`);
  return new Date(epoch * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export function truncateDigest(value: string): string {
  if (!value) return "";
  const [algo, hash] = value.includes(":") ? value.split(":") : ["", value];
  if (hash.length <= 14) return value;
  const shown = `${hash.slice(0, 6)}…${hash.slice(-4)}`;
  return algo ? `${algo}:${shown}` : shown;
}

export function splitRepository(name: string): { namespace: string; repository: string } {
  const index = name.lastIndexOf("/");
  if (index <= 0) return { namespace: "library", repository: name };
  return { namespace: name.slice(0, index), repository: name.slice(index + 1) };
}

export function registryPath(host: string, name: string, reference?: string): string {
  const image = `${host.replace(/^https?:\/\//, "").replace(/\/$/, "")}/${name}`;
  if (!reference) return image;
  return reference.startsWith("sha256:") ? `${image}@${reference}` : `${image}:${reference}`;
}

export function isMultiPlatform(mediaType: string): boolean {
  return mediaType.includes("image.index") || mediaType.includes("manifest.list");
}

export function eventLabel(kind: string): string {
  return kind.replaceAll("_", " ");
}

export function eventResult(kind: string): "success" | "failed" | "info" {
  if (kind.includes("failed") || kind.includes("deleted") || kind.includes("revoked")) return kind.includes("failed") ? "failed" : "info";
  return "success";
}

export function namespaceOf(name: string): string {
  return splitRepository(name).namespace;
}

export function validateRepositoryName(value: string): string | null {
  if (!value) return "Repository name is required.";
  if (value !== value.toLowerCase()) return "Use lowercase letters only.";
  if (value.length > 255) return "Repository name is too long.";
  if (!/^[a-z0-9]+(?:[._-][a-z0-9]+)*(?:\/[a-z0-9]+(?:[._-][a-z0-9]+)*)*$/.test(value)) {
    return "Use lowercase letters, numbers, '.', '_' or '-', separated by '/'.";
  }
  return null;
}

export function splitBytes(value = 0): { value: string; unit: string } {
  const [number, unit] = formatBytes(value).split(" ");
  return { value: number, unit };
}

export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ${minutes % 60}m`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}
