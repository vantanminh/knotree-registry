import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { Repository, RepositoryDetail, Tag, api, friendlyError } from "./api";
import {
  formatBytes,
  isMultiPlatform,
  namespaceOf,
  registryPath,
  splitRepository,
  validateRepositoryName
} from "./format";
import {
  CommandBox,
  ConfirmDialog,
  CopyButton,
  DigestView,
  Drawer,
  EmptyState,
  ErrorState,
  Modal,
  PageHeader,
  RelativeTime,
  RepositoryName,
  SearchInput,
  Skeleton,
  StatusBadge,
  Tabs,
  useToast
} from "./ui";

export function RepositoriesPage({ host }: { host: string }) {
  const [repos, setRepos] = useState<Repository[] | null>(null);
  const [error, setError] = useState("");
  const [create, setCreate] = useState(false);
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const search = params.get("q") ?? "";
  const namespace = params.get("namespace") ?? "";
  const sort = params.get("sort") ?? "updated";
  function load() {
    api<{ repositories: Repository[] }>("/api/v1/repositories")
      .then((result) => setRepos(result.repositories))
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, []);
  const namespaces = useMemo(
    () => Array.from(new Set((repos ?? []).map((repo) => namespaceOf(repo.name)))).sort(),
    [repos]
  );
  const filtered = useMemo(() => {
    const items = (repos ?? []).filter((repo) => {
      const matchesQuery = !search || repo.name.includes(search.toLowerCase()) || (repo.latest_digest ?? "").includes(search);
      const matchesNs = !namespace || namespaceOf(repo.name) === namespace;
      return matchesQuery && matchesNs;
    });
    return [...items].sort((left, right) => {
      if (sort === "name") return left.name.localeCompare(right.name);
      if (sort === "size") return (right.size ?? 0) - (left.size ?? 0);
      return (right.updated_at ?? 0) - (left.updated_at ?? 0);
    });
  }, [repos, search, namespace, sort]);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!repos) return <Skeleton rows={8} />;
  return (
    <>
      <PageHeader
        title="Repositories"
        description="Manage OCI images stored in this registry."
        actions={
          <button className="btn primary" onClick={() => setCreate(true)}>
            New Repository
          </button>
        }
      />
      <div className="filters">
        <SearchInput
          value={search}
          onChange={(value) => {
            params.set("q", value);
            if (!value) params.delete("q");
            setParams(params, { replace: true });
          }}
          placeholder="Search repositories"
        />
        <select
          value={namespace}
          onChange={(event) => {
            params.set("namespace", event.target.value);
            if (!event.target.value) params.delete("namespace");
            setParams(params, { replace: true });
          }}
          aria-label="Namespace"
        >
          <option value="">All namespaces</option>
          {namespaces.map((value) => (
            <option key={value}>{value}</option>
          ))}
        </select>
        <select
          value={sort}
          onChange={(event) => {
            params.set("sort", event.target.value);
            setParams(params, { replace: true });
          }}
          aria-label="Sort"
        >
          <option value="updated">Updated</option>
          <option value="name">Name</option>
          <option value="size">Size</option>
        </select>
      </div>
      <section className="panel">
        {filtered.length ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Repository</th>
                  <th>Namespace</th>
                  <th>Latest tag</th>
                  <th className="num">Image size</th>
                  <th className="num hide-sm">Tags</th>
                  <th>Updated</th>
                  <th>Visibility</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((repo) => (
                  <tr key={repo.name} onClick={() => navigate(`/repositories/${repo.name}`)}>
                    <td>
                      <RepositoryName name={repo.name} />
                    </td>
                    <td>{namespaceOf(repo.name)}</td>
                    <td className="mono">{repo.latest_tag ?? "—"}</td>
                    <td className="num">{formatBytes(repo.size ?? 0)}</td>
                    <td className="num hide-sm">{repo.tag_count ?? 0}</td>
                    <td>
                      <RelativeTime value={repo.updated_at} />
                    </td>
                    <td>
                      <StatusBadge state="neutral" label="Private" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState
            title="No repositories yet"
            text="Push your first image to start using Knotree Registry."
            action={
              <div className="grid-gap" style={{ width: "min(100%, 560px)", textAlign: "left" }}>
                <CommandBox command={`docker login ${host}`} />
                <CommandBox command={`docker tag my-app ${host}/production/my-app:latest`} />
                <CommandBox command={`docker push ${host}/production/my-app:latest`} />
              </div>
            }
          />
        )}
      </section>
      {create && <CreateRepositoryModal host={host} onClose={() => setCreate(false)} />}
    </>
  );
}

function CreateRepositoryModal({ host, onClose }: { host: string; onClose: () => void }) {
  const [namespace, setNamespace] = useState("production");
  const [name, setName] = useState("");
  const preview = `${namespace}/${name}`.replace(/\/+$/, "");
  const error = name ? validateRepositoryName(preview) : null;
  return (
    <Modal
      title="New repository"
      onClose={onClose}
      footer={
        <button className="btn ghost" onClick={onClose}>
          Done
        </button>
      }
    >
      <p>Repositories are created when the first manifest is pushed. Preview the path, then copy the commands.</p>
      <label className="field">
        <span>Namespace</span>
        <input value={namespace} onChange={(event) => setNamespace(event.target.value.toLowerCase())} />
      </label>
      <label className="field">
        <span>Repository name</span>
        <input value={name} onChange={(event) => setName(event.target.value.toLowerCase())} placeholder="api" />
        {error && <small>{error}</small>}
      </label>
      <label className="field">
        <span>Visibility</span>
        <select defaultValue="private">
          <option value="private">Private</option>
          <option value="public" disabled>
            Public (not available)
          </option>
        </select>
      </label>
      <div className="secret-box">{registryPath(host, preview || "namespace/name")}</div>
      {!error && name && (
        <div className="grid-gap">
          <CommandBox command={`docker tag my-app ${registryPath(host, preview)}:latest`} />
          <CommandBox command={`docker push ${registryPath(host, preview)}:latest`} />
        </div>
      )}
    </Modal>
  );
}

export function RepositoryDetailPage({ host }: { host: string }) {
  const params = useParams();
  const [search, setSearch] = useSearchParams();
  const name = params["*"] ?? "";
  const tab = search.get("tab") ?? "tags";
  const [data, setData] = useState<RepositoryDetail | null>(null);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<Tag | null>(null);
  const [inspect, setInspect] = useState<Tag | null>(null);
  const toast = useToast();
  function load() {
    api<RepositoryDetail>(`/api/v1/repositories/${encodeURI(name)}`)
      .then(setData)
      .catch((reason) => setError(friendlyError(reason)));
  }
  useEffect(load, [name]);
  if (error) return <ErrorState message={error} onRetry={load} />;
  if (!data) return <Skeleton rows={8} />;
  const latest = data.tags.find((tag) => tag.tag === "latest") ?? data.tags[0];
  const total = data.tags.reduce((sum, tag) => sum + tag.size, 0);
  const newest = data.tags.reduce((max, tag) => Math.max(max, tag.created_at), 0);
  return (
    <>
      <PageHeader
        title={`${splitRepository(data.name).namespace} / ${splitRepository(data.name).repository}`}
        description={registryPath(host, data.name)}
        actions={
          <>
            <StatusBadge state="neutral" label="Private" />
            <CopyButton value={registryPath(host, data.name)} label="Copy path" />
            <button
              className="btn primary"
              onClick={async () => {
                await navigator.clipboard.writeText(`docker pull ${registryPath(host, data.name, latest?.tag ?? "latest")}`);
                toast("Copied to clipboard");
              }}
            >
              Pull
            </button>
          </>
        }
      />
      <div className="meta-row">
        <div>
          <span>Tags</span>
          <strong>{data.tags.length}</strong>
        </div>
        <div>
          <span>Stored manifests</span>
          <strong>{formatBytes(total)}</strong>
        </div>
        <div>
          <span>Last push</span>
          <strong>
            <RelativeTime value={newest || null} />
          </strong>
        </div>
        <div>
          <span>Latest digest</span>
          <strong>
            <DigestView value={latest?.digest} />
          </strong>
        </div>
      </div>
      <Tabs
        value={tab}
        options={[
          { id: "tags", label: "Tags" },
          { id: "manifests", label: "Manifests" },
          { id: "activity", label: "Activity" },
          { id: "settings", label: "Settings" }
        ]}
        onChange={(id) => {
          search.set("tab", id);
          setSearch(search, { replace: true });
        }}
      />
      {tab === "tags" && (
        <section className="panel">
          {data.tags.length ? (
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>Tag</th>
                    <th>Digest</th>
                    <th>Platform</th>
                    <th className="num">Size</th>
                    <th>Pushed</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {data.tags.map((tag) => (
                    <tr key={tag.tag} onClick={() => setSelected(tag)}>
                      <td>
                        <span className="mono">{tag.tag}</span> {tag.tag === "latest" && <StatusBadge state="info" label="latest" />}
                      </td>
                      <td>
                        <DigestView value={tag.digest} />
                      </td>
                      <td>{isMultiPlatform(tag.media_type) ? "Multi-platform" : "Single-platform"}</td>
                      <td className="num">{formatBytes(tag.size)}</td>
                      <td>
                        <RelativeTime value={tag.created_at} />
                      </td>
                      <td>
                        <span className="row-actions">
                          <CopyButton value={`docker pull ${registryPath(host, data.name, tag.tag)}`} label="Pull" />
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState title="No tags yet" text="Push a tagged manifest to populate this repository." />
          )}
        </section>
      )}
      {tab === "manifests" && (
        <section className="panel">
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Digest</th>
                  <th>Media type</th>
                  <th className="num">Size</th>
                  <th>Layers</th>
                </tr>
              </thead>
              <tbody>
                {data.tags.map((tag) => (
                  <tr key={`${tag.tag}-${tag.digest}`} onClick={() => setInspect(tag)}>
                    <td>
                      <DigestView value={tag.digest} />
                    </td>
                    <td className="mono">{tag.media_type.split(".").slice(-2).join(".")}</td>
                    <td className="num">{formatBytes(tag.size)}</td>
                    <td>{tag.references?.length ?? 0}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
      {tab === "activity" && (
        <section className="panel">
          <EmptyState title="Repository activity" text="Filtered activity for this repository is available in the audit log." action={<Link className="btn" to={`/security/audit?repository=${encodeURIComponent(data.name)}`}>Open audit log</Link>} />
        </section>
      )}
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
  const [confirm, setConfirm] = useState(false);
  return (
    <Drawer title={tag.tag} onClose={onClose}>
      <label className="field">
        <span>Digest</span>
        <DigestView value={tag.digest} />
      </label>
      <label className="field">
        <span>Media type</span>
        <code>{tag.media_type}</code>
      </label>
      <label className="field">
        <span>Compressed size</span>
        <strong>{formatBytes(tag.size)}</strong>
      </label>
      <label className="field">
        <span>Pushed</span>
        <RelativeTime value={tag.created_at} />
      </label>
      <CommandBox command={`docker pull ${registryPath(host, repository, tag.tag)}`} />
      <p style={{ color: "var(--muted)", fontSize: 13 }}>Digest references always resolve to this exact image version.</p>
      <CommandBox command={`docker pull ${registryPath(host, repository, tag.digest)}`} />
      <div className="header-actions">
        <button className="btn" onClick={onInspect}>
          View manifest
        </button>
        <button className="btn danger" onClick={() => setConfirm(true)}>
          Delete tag
        </button>
      </div>
      {confirm && (
        <ConfirmDialog
          title="Delete tag?"
          text="Deleting this manifest removes its repository reference. Unreferenced data may be removed later by garbage collection."
          confirmLabel="Delete tag"
          confirmValue={`${repository}:${tag.tag}`}
          danger
          onClose={() => setConfirm(false)}
          onConfirm={() => setConfirm(false)}
        />
      )}
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
  return (
    <Modal title="Manifest inspector" onClose={onClose} wide footer={<CopyButton value={payload} label="Copy JSON" />}>
      <h3>Overview</h3>
      <p className="mono">{tag.digest}</p>
      <h3>Layers</h3>
      {(tag.references ?? []).length ? (
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
              {(tag.references ?? []).map((ref) => (
                <tr key={ref.digest}>
                  <td>
                    <DigestView value={ref.digest} />
                  </td>
                  <td className="mono">{ref.media_type}</td>
                  <td className="num">{formatBytes(ref.size)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <p>No referenced descriptors are recorded for this tag.</p>
      )}
      <h3>Raw JSON</h3>
      <pre className="secret-box">{payload}</pre>
    </Modal>
  );
}

function RepositorySettings({ name, host }: { name: string; host: string }) {
  return (
    <div className="grid-gap">
      <section className="panel panel-pad grid-gap">
        <h2>General</h2>
        <label className="field">
          <span>Name</span>
          <input value={name} readOnly />
        </label>
        <label className="field">
          <span>Registry path</span>
          <div className="secret-box">{registryPath(host, name)}</div>
        </label>
        <label className="field">
          <span>Visibility</span>
          <select defaultValue="private">
            <option>Private</option>
          </select>
        </label>
      </section>
      <section className="danger-zone">
        <h3>Danger zone</h3>
        <p>Permanently delete repository metadata and schedule unreferenced content for garbage collection.</p>
        <p style={{ marginTop: 8, color: "var(--muted)" }}>Repository deletion is not exposed on this control plane yet. Remove manifests through the OCI API, then run garbage collection.</p>
      </section>
    </div>
  );
}

export function NamespacesPage() {
  const [repos, setRepos] = useState<Repository[] | null>(null);
  const [error, setError] = useState("");
  const navigate = useNavigate();
  useEffect(() => {
    api<{ repositories: Repository[] }>("/api/v1/repositories")
      .then((result) => setRepos(result.repositories))
      .catch((reason) => setError(friendlyError(reason)));
  }, []);
  if (error) return <ErrorState message={error} />;
  if (!repos) return <Skeleton />;
  const groups = new Map<string, Repository[]>();
  for (const repo of repos) {
    const ns = namespaceOf(repo.name);
    groups.set(ns, [...(groups.get(ns) ?? []), repo]);
  }
  return (
    <>
      <PageHeader title="Namespaces" description="Namespaces group repositories, tokens, and access." />
      <section className="panel">
        {groups.size ? (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>Namespace</th>
                  <th className="num">Repositories</th>
                  <th className="num">Storage</th>
                </tr>
              </thead>
              <tbody>
                {[...groups.entries()].map(([ns, items]) => (
                  <tr key={ns} onClick={() => navigate(`/repositories?namespace=${ns}`)}>
                    <td className="repo-primary">{ns}</td>
                    <td className="num">{items.length}</td>
                    <td className="num">{formatBytes(items.reduce((sum, item) => sum + (item.size ?? 0), 0))}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyState title="No namespaces yet" text="Namespaces appear after the first image is pushed." />
        )}
      </section>
    </>
  );
}
