import { ReactNode } from "react";
import {
  Activity,
  KeyRound,
  LogIn,
  ShieldCheck,
  ShieldOff,
  Tag,
  Trash2,
  Unlink,
  Upload,
  Webhook
} from "lucide-react";
import { AuditEvent } from "./api";
import { eventLabel } from "./format";
import { RelativeTime } from "./ui";

type Meta = { icon: ReactNode; tone: "" | "ok" | "fail" | "accent" | "warn"; verb: string };

const meta: Record<string, Meta> = {
  manifest_pushed: { icon: <Upload />, tone: "accent", verb: "Pushed" },
  tag_updated: { icon: <Tag />, tone: "accent", verb: "Updated tag" },
  manifest_deleted: { icon: <Trash2 />, tone: "warn", verb: "Deleted manifest" },
  token_created: { icon: <KeyRound />, tone: "ok", verb: "Created access token" },
  token_revoked: { icon: <KeyRound />, tone: "warn", verb: "Revoked access token" },
  grant_revoked: { icon: <Unlink />, tone: "warn", verb: "Revoked Cloud grant" },
  password_changed: { icon: <ShieldCheck />, tone: "", verb: "Changed password" },
  two_factor_enabled: { icon: <ShieldCheck />, tone: "ok", verb: "Enabled two-step verification" },
  two_factor_disabled: { icon: <ShieldOff />, tone: "warn", verb: "Disabled two-step verification" },
  login_succeeded: { icon: <LogIn />, tone: "", verb: "Signed in" },
  login_failed: { icon: <LogIn />, tone: "fail", verb: "Sign-in failed" },
  garbage_collection: { icon: <Trash2 />, tone: "", verb: "Ran garbage collection" },
  webhook_created: { icon: <Webhook />, tone: "ok", verb: "Added webhook" },
  webhook_disabled: { icon: <Webhook />, tone: "warn", verb: "Disabled webhook" }
};

export function eventMeta(kind: string): Meta {
  return meta[kind] ?? { icon: <Activity />, tone: "", verb: eventLabel(kind) };
}

export function eventFailed(kind: string) {
  return kind.includes("failed");
}

export function eventResource(event: AuditEvent): string | null {
  if (!event.repository) return null;
  return event.tag ? `${event.repository}:${event.tag}` : event.repository;
}

function dayLabel(epoch: number): string {
  const date = new Date(epoch * 1000);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (date.toDateString() === today.toDateString()) return "Today";
  if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
  return date.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

export function groupByDay<T>(items: T[], at: (item: T) => number): { day: string; items: T[] }[] {
  const groups: { day: string; items: T[] }[] = [];
  for (const item of items) {
    const day = dayLabel(at(item));
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.items.push(item);
    else groups.push({ day, items: [item] });
  }
  return groups;
}

/** Events in reverse-chronological order, grouped under day headings. */
export function ActivityFeed({
  events,
  grouped = true,
  onSelect
}: {
  events: AuditEvent[];
  grouped?: boolean;
  onSelect?: (event: AuditEvent) => void;
}) {
  const groups = grouped ? groupByDay(events, (event) => event.occurred_at) : [{ day: "", items: events }];
  return (
    <div className="feed">
      {groups.map((group) => (
        <div key={group.day || "all"}>
          {group.day && <div className="feed-day">{group.day}</div>}
          {group.items.map((event) => (
            <FeedItem key={event.id} event={event} onSelect={onSelect} />
          ))}
        </div>
      ))}
    </div>
  );
}

function FeedItem({ event, onSelect }: { event: AuditEvent; onSelect?: (event: AuditEvent) => void }) {
  const info = eventMeta(event.kind);
  const resource = eventResource(event);
  return (
    <div
      className={`feed-item ${onSelect ? "clickable" : ""}`}
      onClick={onSelect ? () => onSelect(event) : undefined}
      onKeyDown={onSelect ? (e) => e.key === "Enter" && onSelect(event) : undefined}
      tabIndex={onSelect ? 0 : undefined}
      role={onSelect ? "button" : undefined}
    >
      <span className={`feed-icon ${info.tone}`}>{info.icon}</span>
      <div className="feed-main">
        <div className="title">
          <b>{info.verb}</b>
          {resource && (
            <>
              {" "}
              <span className="mono">{resource}</span>
            </>
          )}
        </div>
        <div className="sub">{event.actor ? `by ${event.actor}` : "by system"}</div>
      </div>
      <RelativeTime value={event.occurred_at} />
    </div>
  );
}

/** Counts of events per day for the last `days` days, oldest first. */
export function dailyCounts(events: AuditEvent[], days: number) {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - (days - 1));
  const buckets = Array.from({ length: days }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    return { date, pushes: 0, other: 0 };
  });
  for (const event of events) {
    const index = Math.floor((event.occurred_at * 1000 - start.getTime()) / 86_400_000);
    if (index < 0 || index >= days) continue;
    if (event.kind === "manifest_pushed" || event.kind === "tag_updated") buckets[index].pushes += 1;
    else buckets[index].other += 1;
  }
  return buckets;
}
