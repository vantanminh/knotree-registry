import { useMemo, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { ArrowRight, Box, Boxes, ChevronRight, Clock, FileJson, HardDrive, Lock, Plus, Tags as TagsIcon } from "lucide-react";
import { AuditEvent, Repository, RepositoryDetail, Tag, useResource } from "./api";
import { ActivityFeed } from "./events";
import { formatBytes, isMultiPlatform, namespaceOf, registryPath, splitBytes, splitRepository, validateRepositoryName } from "./format";
import { EventDrawer } from "./pages-overview";
import { useSession } from "./session";
import {
  Callout,
  CommandBox,
  CopyButton,
  DigestView,
  Drawer,
  EmptyState,
  ErrorState,
  Modal,
  PageHeader,
  Panel,
  RelativeTime,
  RepositoryName,
  SearchInput,
  Segmented,
  Skeleton,
  Stats,
  StatusBadge,
  TagPill,
  Tabs
} from "./ui";

type Sort = "updated" | "name" | "size";

export function RepositoriesPage() {
  const { host } = useSession();
  const { data, error, reload } = useResource<{ repositories: Repository[] }>("/api/v1/repositories");
  const [create, setCreate] = useState(false);
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const search = params.get("q") ?? "";
  const namespace = params.get("namespace") ?? "";
  const sort = (params.get("sort") as Sort) ?? "updated";
  const repos = data?.repositories;

  function setParam(key: string, value: string) {
    const next = new URLSearchParams(params);
    if (value) next.set(key, value);
    else next.delete(key);
    setParams(next, { replace: true });
  }

  const namespaces = useMemo(() => {
    const map = new Map<string, number>();
    for (const repo of repos ?? []) map.set(namespaceOf(repo.name), (map.get(namespaceOf(repo.name)) ?? 0) + 1);
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [repos]);
  const maxSize = Math.max(1, ...(repos ?? []).map((repo) => repo.size ?? 0));
  const filtered = useMemo(() => {
    const q = search.toLowerCase();
    const items = (repos ?? []).filter(
      (repo) =>
        (!q || repo.name.includes(q) || (repo.latest_digest ?? "").includes(q) || (repo.latest_tag ?? "").includes(q)) &&
        (!namespace || namespaceOf(repo.name) === namespace)
    );
    return [...items].sort((left, right) => {
      if (sort === "name") return left.name.localeCompare(right.name);
      if (sort === "size") return (right.size ?? 0) - (left.size ?? 0);
      return (right.updated_at ?? 0) - (left.updated_at ?? 0);
    });
  }, [repos, search, namespace, sort]);

  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!repos) return <Skeleton rows={8} />;

  return (
    <>
      <PageHeader
        title="Repositories"
        description="Every image repository in this registry. Repositories are created by their first push."
        actions={
          <button className="btn primary" onClick={() => setCreate(true)}>
            <Plus size={14} /> New repository
          </button>
        }
      />
      {repos.length > 0 && (
        <div className="toolbar">
          <SearchInput value={search} onChange={(value) => setParam("q", value)} placeholder="Filter by name, tag or digest" />
          {namespaces.length > 1 && namespaces.length <= 6 ? (
            <div className="filter-chips">
              <button className={!namespace ? "filter-chip active" : "filter-chip"} onClick={() => setParam("namespace", "")}>
                All
              </button>
              {namespaces.map(([ns, count]) => (
                <button key={ns} className={namespace === ns ? "filter-chip active" : "filter-chip"} onClick={() => setParam("namespace", namespace === ns ? "" : ns)}>
                  {ns} <span className="count">{count}</span>
                </button>
              ))}
            </div>
          ) : (
            namespaces.length > 1 && (
              <select className="select-sm" value={namespace} onChange={(event) => setParam("namespace", event.target.value)} aria-label="Namespace">
                <option value="">All namespaces</option>
                {namespaces.map(([ns, count]) => (
                  <option key={ns} value={ns}>
                    {ns} ({count})
                  </option>
                ))}
              </select>
            )
          )}
          <span className="spacer" />
          <Segmented
            label="Sort"
            value={sort}
            onChange={(value) => setParam("sort", value === "updated" ? "" : value)}
            options={[
              { id: "updated", label: "Recent" },
              { id: "name", label: "Name" },
              { id: "size", label: "Size" }
            ]}
          />
        </div>
      )}
      <section className="panel">
        {filtered.length ? (
          <>
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>Repository</th>
                    <th>Latest tag</th>
                    <th className="num hide-sm">Tags</th>
                    <th className="num">Size</th>
                    <th className="num hide-sm">Updated</th>
                    <th className="shrink" aria-label="Actions" />
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((repo) => (
                    <tr key={repo.name} className="clickable" onClick={() => navigate(`/repositories/${repo.name}`)}>
                      <td>
                        <RepositoryName name={repo.name} />
                      </td>
                      <td>
                        <TagPill tag={repo.latest_tag} />
                      </td>
                      <td className="num hide-sm">{repo.tag_count ?? 0}</td>
                      <td className="num">
                        <span className="inline-meter">
                          <span className="tnum">{formatBytes(repo.size ?? 0)}</span>
                          <span className="track hide-sm">
                            <span style={{ width: `${((repo.size ?? 0) / maxSize) * 100}%` }} />
                          </span>
                        </span>
                      </td>
                      <td className="num hide-sm muted">
                        <RelativeTime value={repo.updated_at} />
                      </td>
                      <td>
                        <div className="row-actions">
                          <CopyButton
                            value={`docker pull ${registryPath(host, repo.name, repo.latest_tag ?? "latest")}`}
                            title="Copy pull command"
                          />
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="panel-foot">
              <span>
                {filtered.length} of {repos.length} repositories
              </span>
              <span>All repositories are private</span>
            </div>
          </>
        ) : repos.length ? (
          <EmptyState
            icon={<Box size={18} />}
            title="No matching repositories"
            text="Nothing matches the current filter."
            action={
              <button className="btn" onClick={() => setParams({}, { replace: true })}>
                Clear filters
              </button>
            }
          />
        ) : (
          <div className="panel-body">
            <EmptyState icon={<Box size={18} />} title="No repositories yet" text="Push your first image and the repository appears here." />
            <div className="stack-sm" style={{ maxWidth: 560, margin: "0 auto 16px" }}>
              <CommandBox command={`docker login ${host}`} />
              <CommandBox command={`docker tag my-app ${host}/production/my-app:latest`} />
              <CommandBox command={`docker push ${host}/production/my-app:latest`} />
            </div>
          </div>
        )}
      </section>
      {create && <CreateRepositoryModal host={host} onClose={() => setCreate(false)} />}
    </>
  );
}

function CreateRepositoryModal({ host, onClose }: { host: string; onClose: () => void }) {
  const [namespace, setNamespace] = useState("production");
  const [name, setName] = useState("");
  const preview = [namespace, name].filter(Boolean).join("/");
  const error = name ? validateRepositoryName(preview) : null;
  const ready = name && !error;
  return (
    <Modal
      title="New repository"
      description="Repositories are created by the first push. Choose a path, then run the commands."
      onClose={onClose}
      footer={
        <button className="btn primary" onClick={onClose}>
          Done
        </button>
      }
    >
      <div className="choices">
        <label className="field">
          <span>Namespace</span>
          <input value={namespace} onChange={(event) => setNamespace(event.target.value.toLowerCase())} spellCheck={false} />
        </label>
        <label className="field">
          <span>Name</span>
          <input value={name} onChange={(event) => setName(event.target.value.toLowerCase())} placeholder="api" autoFocus spellCheck={false} />
        </label>
      </div>
      {error && <div className="error-text" style={{ color: "var(--danger)", fontSize: 12.5, marginTop: -8 }}>{error}</div>}
      <div className="secret">
        <Lock size={14} className="muted" />
        <code>{registryPath(host, preview || "namespace/name")}</code>
        <StatusBadge state="neutral" label="Private" />
      </div>
      <div className="stack-sm" style={{ opacity: ready ? 1 : 0.45, transition: "opacity 150ms" }}>
        <CommandBox label="1 · Tag a local image" command={`docker tag my-app ${registryPath(host, ready ? preview : "namespace/name")}:latest`} />
        <CommandBox label="2 · Push it" command={`docker push ${registryPath(host, ready ? preview : "namespace/name")}:latest`} />
      </div>
    </Modal>
  );
}

export function RepositoryDetailPage() {
  const { host } = useSession();
  const params = useParams();
  const [search, setSearch] = useSearchParams();
  const name = params["*"] ?? "";
  const tab = search.get("tab") ?? "tags";
  const { data, error, reload } = useResource<RepositoryDetail>(`/api/v1/repositories/${encodeURI(name)}`);
  const [selected, setSelected] = useState<Tag | null>(null);
  const [inspect, setInspect] = useState<Tag | null>(null);
  const [filter, setFilter] = useState("");

  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton rows={8} stats />;
  const parts = splitRepository(data.name);
  const tags = [...data.tags].sort((a, b) => b.created_at - a.created_at);
  const latest = data.tags.find((tag) => tag.tag === "latest") ?? tags[0];
  const total = data.tags.reduce((sum, tag) => sum + tag.size, 0);
  const newest = tags[0]?.created_at ?? null;
  const digests = new Set(data.tags.map((tag) => tag.digest)).size;
  const shown = tags.filter((tag) => !filter || tag.tag.includes(filter.toLowerCase()) || tag.digest.includes(filter));
  const setTab = (id: string) => {
    const next = new URLSearchParams(search);
    if (id === "tags") next.delete("tab");
    else next.set("tab", id);
    setSearch(next, { replace: true });
  };

  return (
    <>
      <PageHeader
        eyebrow={
          <Link to={`/repositories?namespace=${encodeURIComponent(parts.namespace)}`} className="inline-link">
            {parts.namespace}
          </Link>
        }
        title={parts.repository}
        badge={<StatusBadge state="neutral" label={<><Lock size={11} /> Private</>} />}
        actions={
          <>
            <CopyButton className="btn" value={registryPath(host, data.name)} label="Copy image path" />

          </>
        }
      />
      <div style={{ marginBottom: 16 }}>
        <CommandBox command={`docker pull ${registryPath(host, data.name, latest?.tag ?? "latest")}`} />
      </div>
      <Stats
        items={[
          { label: "Tags", icon: <TagsIcon />, value: data.tags.length, detail: `${digests} unique digest${digests === 1 ? "" : "s"}` },
          { label: "Manifest size", icon: <HardDrive />, ...splitBytes(total), detail: "Sum across tags" },
          { label: "Last push", icon: <Clock />, value: <RelativeTime value={newest} />, detail: tags[0] ? <>tag <span className="mono">{tags[0].tag}</span></> : "No pushes yet" },
          { label: "Latest digest", icon: <FileJson />, value: <span style={{ fontSize: 15 }}><DigestView value={latest?.digest} /></span>, detail: latest ? <>from <span className="mono">{latest.tag}</span></> : "—" }
        ]}
      />
      <Tabs
        value={tab}
        options={[
          { id: "tags", label: "Tags", count: data.tags.length },
          { id: "manifests", label: "Manifests", count: digests },
          { id: "activity", label: "Activity" },
          { id: "settings", label: "Settings" }
        ]}
        onChange={setTab}
      />
      {tab === "tags" && (
        <>
          {data.tags.length > 6 && (
            <div className="toolbar">
              <SearchInput value={filter} onChange={setFilter} placeholder="Filter tags" />
            </div>
          )}
          <section className="panel">
            {shown.length ? (
              <div className="table-wrap">
                <table className="data">
                  <thead>
                    <tr>
                      <th>Tag</th>
                      <th>Digest</th>
                      <th className="hide-sm">Type</th>
                      <th className="num">Size</th>
                      <th className="num hide-sm">Pushed</th>
                      <th className="shrink" aria-label="Actions" />
                    </tr>
                  </thead>
                  <tbody>
                    {shown.map((tag) => (
                      <tr key={tag.tag} className="clickable" onClick={() => setSelected(tag)}>
                        <td>
                          <TagPill tag={tag.tag} />
                        </td>
                        <td>
                          <DigestView value={tag.digest} />
                        </td>
                        <td className="hide-sm muted">{isMultiPlatform(tag.media_type) ? "Multi-platform index" : "Image manifest"}</td>
                        <td className="num">{formatBytes(tag.size)}</td>
                        <td className="num hide-sm muted">
                          <RelativeTime value={tag.created_at} />
                        </td>
                        <td>
                          <div className="row-actions">
                            <CopyButton value={`docker pull ${registryPath(host, data.name, tag.tag)}`} label="Pull" />
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <EmptyState icon={<TagsIcon size={18} />} title={filter ? "No matching tags" : "No tags yet"} text={filter ? "Try another tag name or digest." : "Push a tagged image to populate this repository."} />
            )}
          </section>
        </>
      )}
      {tab === "manifests" && <ManifestList tags={tags} onInspect={setInspect} />}
      {tab === "activity" && <RepositoryActivity name={data.name} />}
      {tab === "settings" && <RepositorySettings name={data.name} host={host} />}
      {selected && (
        <TagDrawer
          host={host}
          repository={data.name}
          tag={selected}
          onClose={() => setSelected(null)}
          onInspect={() => {
            setInspect(selected);
            setSelected(null);
          }}
        />
      )}
      {inspect && <ManifestInspector tag={inspect} onClose={() => setInspect(null)} />}
    </>
  );
}

function ManifestList({ tags, onInspect }: { tags: Tag[]; onInspect: (tag: Tag) => void }) {
  const byDigest = new Map<string, Tag[]>();
  for (const tag of tags) byDigest.set(tag.digest, [...(byDigest.get(tag.digest) ?? []), tag]);
  return (
    <section className="panel">
      <div className="table-wrap">
        <table className="data">
          <thead>
            <tr>
              <th>Digest</th>
              <th>Tags</th>
              <th className="hide-sm">Media type</th>
              <th className="num">References</th>
              <th className="num">Size</th>
            </tr>
          </thead>
          <tbody>
            {[...byDigest.entries()].map(([digest, group]) => (
              <tr key={digest} className="clickable" onClick={() => onInspect(group[0])}>
                <td>
                  <DigestView value={digest} />
                </td>
                <td>
                  <span className="row" style={{ gap: 4, flexWrap: "nowrap" }}>
                    {group.slice(0, 3).map((tag) => (
                      <TagPill key={tag.tag} tag={tag.tag} />
                    ))}
                    {group.length > 3 && <span className="muted">+{group.length - 3}</span>}
                  </span>
                </td>
                <td className="mono muted hide-sm">{group[0].media_type.replace("application/vnd.", "")}</td>
                <td className="num">{group[0].references?.length ?? 0}</td>
                <td className="num">{formatBytes(group[0].size)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function RepositoryActivity({ name }: { name: string }) {
  const { data, error, reload } = useResource<{ events: AuditEvent[] }>("/api/v1/audit?limit=200");
  const [selected, setSelected] = useState<AuditEvent | null>(null);
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton rows={4} />;
  const events = data.events.filter((event) => event.repository === name);
  return (
    <section className="panel" style={{ paddingBottom: 8 }}>
      {events.length ? (
        <ActivityFeed events={events} onSelect={setSelected} />
      ) : (
        <EmptyState icon={<Clock size={18} />} title="No recent activity" text="Pushes and tag changes for this repository will appear here." />
      )}
      {selected && <EventDrawer event={selected} onClose={() => setSelected(null)} />}
    </section>
  );
}

function TagDrawer({
  host,
  repository,
  tag,
  onClose,
  onInspect
}: {
  host: string;
  repository: string;
  tag: Tag;
  onClose: () => void;
  onInspect: () => void;
}) {
  return (
    <Drawer
      eyebrow={<span className="mono">{repository}</span>}
      title={<span className="mono">:{tag.tag}</span>}
      onClose={onClose}
      actions={
        <button className="btn sm" onClick={onInspect}>
          <FileJson size={13} /> Manifest
        </button>
      }
    >
      <dl className="kv">
        <dt>Digest</dt>
        <dd>
          <DigestView value={tag.digest} />
        </dd>
        <dt>Type</dt>
        <dd>{isMultiPlatform(tag.media_type) ? "Multi-platform index" : "Image manifest"}</dd>
        <dt>Media type</dt>
        <dd className="mono" style={{ fontSize: 12 }}>{tag.media_type}</dd>
        <dt>Size</dt>
        <dd>{formatBytes(tag.size)}</dd>
        <dt>Pushed</dt>
        <dd>
          <RelativeTime value={tag.created_at} />
        </dd>
        <dt>References</dt>
        <dd>{tag.references?.length ?? 0} descriptors</dd>
      </dl>
      <div className="stack-sm">
        <CommandBox label="Pull by tag" command={`docker pull ${registryPath(host, repository, tag.tag)}`} />
        <CommandBox label="Pull by digest — always this exact image" command={`docker pull ${registryPath(host, repository, tag.digest)}`} />
      </div>
    </Drawer>
  );
}

function ManifestInspector({ tag, onClose }: { tag: Tag; onClose: () => void }) {
  const payload = JSON.stringify(
    {
      tag: tag.tag,
      digest: tag.digest,
      mediaType: tag.media_type,
      size: tag.size,
      created_at: tag.created_at,
      references: tag.references ?? [],
      subject: tag.subject ?? null
    },
    null,
    2
  );
  const refs = tag.references ?? [];
  return (
    <Modal
      title="Manifest"
      description={<span className="mono">{tag.digest}</span>}
      onClose={onClose}
      wide
      footer={
        <>
          <span className="left muted" style={{ fontSize: 12.5 }}>
            {refs.length} references · {formatBytes(refs.reduce((sum, ref) => sum + ref.size, 0))}
          </span>
          <CopyButton className="btn" value={payload} label="Copy JSON" />
        </>
      }
    >
      {refs.length ? (
        <div className="panel">
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Digest</th>
                  <th>Media type</th>
                  <th className="num">Size</th>
                </tr>
              </thead>
              <tbody>
                {refs.map((ref) => (
                  <tr key={ref.digest}>
                    <td>
                      <DigestView value={ref.digest} />
                    </td>
                    <td className="mono muted" style={{ fontSize: 12 }}>{ref.media_type.replace("application/vnd.", "")}</td>
                    <td className="num">{formatBytes(ref.size)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        <p className="muted">No referenced descriptors are recorded for this manifest.</p>
      )}
      <pre className="code-block">{payload}</pre>
    </Modal>
  );
}

function RepositorySettings({ name, host }: { name: string; host: string }) {
  return (
    <div className="stack-lg">
      <Panel title="General">
        <div className="panel-body">
          <dl className="kv">
            <dt>Name</dt>
            <dd className="mono">{name}</dd>
            <dt>Registry path</dt>
            <dd>
              <span className="row" style={{ gap: 4 }}>
                <span className="mono">{registryPath(host, name)}</span>
                <CopyButton value={registryPath(host, name)} />
              </span>
            </dd>
            <dt>Visibility</dt>
            <dd>
              <StatusBadge state="neutral" label="Private" /> <span className="muted" style={{ fontSize: 12.5, marginLeft: 6 }}>Public repositories are not available on this instance.</span>
            </dd>
          </dl>
        </div>
      </Panel>
      <section className="danger-zone">
        <div className="panel-head bordered">
          <div>
            <h2>Delete repository</h2>
            <p>Not available from the dashboard yet.</p>
          </div>
        </div>
        <div className="panel-body">
          <Callout tone="info">
            Delete manifests through the OCI API (<code>DELETE /v2/{name}/manifests/&lt;digest&gt;</code>), then run garbage collection to free the
            storage they used.
          </Callout>
        </div>
      </section>
    </div>
  );
}

export function NamespacesPage() {
  const { data, error, reload } = useResource<{ repositories: Repository[] }>("/api/v1/repositories");
  if (error) return <ErrorState message={error} onRetry={reload} />;
  if (!data) return <Skeleton />;
  const groups = new Map<string, Repository[]>();
  for (const repo of data.repositories) {
    const ns = namespaceOf(repo.name);
    groups.set(ns, [...(groups.get(ns) ?? []), repo]);
  }
  const entries = [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  return (
    <>
      <PageHeader title="Namespaces" description="The first path segment of a repository. Use them to group images by team, product or environment." />
      {entries.length ? (
        <div className="card-grid">
          {entries.map(([ns, items]) => {
            const size = items.reduce((sum, item) => sum + (item.size ?? 0), 0);
            const updated = Math.max(...items.map((item) => item.updated_at ?? 0));
            return (
              <Link key={ns} className="ns-card" to={`/repositories?namespace=${encodeURIComponent(ns)}`}>
                <div className="ns-card-head">
                  <span className="repo-icon"><Boxes /></span>
                  <strong>{ns}</strong>
                  <ChevronRight size={16} className="chev" />
                </div>
                <div className="meta-strip">
                  <div>
                    <span>Repositories</span>
                    <strong>{items.length}</strong>
                  </div>
                  <div>
                    <span>Storage</span>
                    <strong>{formatBytes(size)}</strong>
                  </div>
                  <div>
                    <span>Updated</span>
                    <strong><RelativeTime value={updated || null} /></strong>
                  </div>
                </div>
                <div className="repos">
                  {items.slice(0, 4).map((item) => (
                    <span className="tag-pill" key={item.name}>{splitRepository(item.name).repository}</span>
                  ))}
                  {items.length > 4 && <span className="muted" style={{ fontSize: 12, alignSelf: "center" }}>+{items.length - 4} more</span>}
                </div>
              </Link>
            );
          })}
        </div>
      ) : (
        <section className="panel">
          <EmptyState
            icon={<Boxes size={18} />}
            title="No namespaces yet"
            text="Namespaces appear after the first image is pushed."
            action={
              <Link className="btn" to="/repositories">
                Go to repositories <ArrowRight size={14} />
              </Link>
            }
          />
        </section>
      )}
    </>
  );
}
