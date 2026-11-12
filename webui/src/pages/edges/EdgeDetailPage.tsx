// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { shadows, edges, tags } from "../../api/client";
import type { AssignedTag, StateMetadata, TagAssignBody, TaskStatus } from "../../api/types";
import { Link, useParams } from "@tanstack/react-router";
import { formatTimestamp, getTagColor, unescapeNewlines } from "../../lib/utils";
import { useCallback, useMemo, useState } from "react";
import { ConfirmDialog } from "../../components/ui/ConfirmDialog";

function formatUptime(seconds: number): string {
    const d = Math.floor(seconds / 86400);
    const h = Math.floor((seconds % 86400) / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    if (d > 0) return `${d}d ${h}h ${m}m`;
    if (h > 0) return `${h}h ${m}m`;
    return `${m}m`;
}

function bytesToGiB(bytes: number): string {
    return (bytes / 1024 / 1024 / 1024).toFixed(2);
}
import { stringify as tomlStringify } from "smol-toml";

type PreviewFormat = "json" | "toml";

export function EdgeDetailPage() {
    const { edgeId } = useParams({ from: "/edges/$edgeId" });
    const queryClient = useQueryClient();
    const [assignTagId, setAssignTagId] = useState("");
    const [previewFormat, setPreviewFormat] = useState<PreviewFormat>("json");
    const [copied, setCopied] = useState(false);
    const [unassignConfirm, setUnassignConfirm] = useState<{ tagId: string; tagName: string } | null>(null);

    const shadowQuery = useQuery({
        queryKey: ["shadows", edgeId],
        queryFn: () => shadows.get(edgeId),
        refetchInterval: 10000,
    });

    const tagsQuery = useQuery({
        queryKey: ["edges", edgeId, "tags"],
        queryFn: () => edges.getTags(edgeId),
        refetchInterval: 10000,
    });

    const previewQuery = useQuery({
        queryKey: ["edges", edgeId, "preview"],
        queryFn: () => edges.previewConfig(edgeId),
    });

    const allTagsQuery = useQuery({
        queryKey: ["tags"],
        queryFn: tags.list,
    });

    const assignMutation = useMutation({
        mutationFn: (body: TagAssignBody) => tags.assign(body),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ["edges", edgeId] });
            setAssignTagId("");
        },
        onError: (err) => alert(`Failed to assign tag: ${err.message}`),
    });

    const unassignMutation = useMutation({
        mutationFn: (body: TagAssignBody) => tags.unassign(body),
        onSuccess: () =>
            queryClient.invalidateQueries({ queryKey: ["edges", edgeId] }),
        onError: (err) => alert(`Failed to unassign tag: ${err.message}`),
    });

    const shadow = shadowQuery.data;
    const edgeTags: AssignedTag[] = tagsQuery.data ?? [];

    // Tags not yet assigned to this edge
    const assignedTagIds = new Set(edgeTags.map((t) => t.id));
    const availableTags =
        allTagsQuery.data?.filter((t) => !assignedTagIds.has(t.id)) ?? [];

    // id -> display_name lookup for assigned tag badges and epoch display
    const tagDisplayNames = useMemo(() => {
        const map = new Map<string, string>();
        for (const t of allTagsQuery.data ?? []) {
            if (t.display_name) map.set(t.id, t.display_name);
        }
        return map;
    }, [allTagsQuery.data]);

    const previewText = useMemo(() => {
        const data = previewQuery.data;
        if (!data) return "";
        if (previewFormat === "json") {
            return JSON.stringify(data, null, 2);
        }
        try {
            return tomlStringify(data);
        } catch {
            return `# Failed to convert to TOML\n${JSON.stringify(data, null, 2)}`;
        }
    }, [previewQuery.data, previewFormat]);

    const handleCopy = useCallback(() => {
        if (!previewText) return;
        navigator.clipboard.writeText(previewText).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        }).catch(() => { /* clipboard permission denied */ });
    }, [previewText]);

    return (
        <div className="space-y-6">
            <h1 className="text-2xl font-bold text-gray-900">Edge: {edgeId}</h1>

            {/* Shadow info */}
            <section className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                <h2 className="mb-3 text-lg font-semibold text-gray-900">
                    Shadow State
                </h2>
                {shadowQuery.isLoading ? (
                    <p className="text-sm text-gray-500">Loading...</p>
                ) : !shadow ? (
                    <p className="text-sm text-gray-400">No shadow found</p>
                ) : (
                    <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                        <dt className="font-medium text-gray-500">Status</dt>
                        <dd className="text-gray-900">
                            {shadow.last_report?.status ?? "Unknown"}
                        </dd>
                        <dt className="font-medium text-gray-500">Place</dt>
                        <dd className="text-gray-900">
                            {shadow.last_report?.place_id ?? "unknown"}
                        </dd>
                        <dt className="font-medium text-gray-500">Last Seen</dt>
                        <dd className="text-gray-900">{shadow.last_seen != null ? formatTimestamp(shadow.last_seen) : "—"}</dd>
                        {shadow.last_report?.system_info && (
                            <>
                                <dt className="font-medium text-gray-500">OS</dt>
                                <dd className="text-gray-900">
                                    {shadow.last_report.system_info.os_name}{" "}
                                    {shadow.last_report.system_info.os_version}
                                </dd>
                                <dt className="font-medium text-gray-500">CPU Usage</dt>
                                <dd className="text-gray-900">
                                    {shadow.last_report.system_info.global_cpu_usage.toFixed(1)}%
                                </dd>
                                <dt className="font-medium text-gray-500">Memory Used</dt>
                                <dd className="text-gray-900">
                                    {bytesToGiB(shadow.last_report.system_info.used_memory)} GiB
                                </dd>
                                <dt className="font-medium text-gray-500">Memory Available</dt>
                                <dd className="text-gray-900">
                                    {bytesToGiB(shadow.last_report.system_info.available_memory)} GiB
                                </dd>
                                <dt className="font-medium text-gray-500">Memory Total</dt>
                                <dd className="text-gray-900">
                                    {bytesToGiB(shadow.last_report.system_info.total_memory)} GiB
                                </dd>
                                <dt className="col-span-2">
                                    {(() => {
                                        const total = shadow.last_report.system_info.total_memory;
                                        const used = shadow.last_report.system_info.used_memory;
                                        const available = shadow.last_report.system_info.available_memory;
                                        const usedPct = total > 0 ? (used / total) * 100 : 0;
                                        const availPct = total > 0 ? (available / total) * 100 : 0;
                                        return (
                                            <div className="flex h-5 w-full overflow-hidden rounded text-xs">
                                                <div
                                                    className="flex items-center justify-center bg-indigo-500 text-white"
                                                    style={{ width: `${usedPct}%` }}
                                                    title={`Used: ${bytesToGiB(used)} GiB`}
                                                >
                                                    {usedPct >= 15 ? "Used" : ""}
                                                </div>
                                                <div
                                                    className="flex items-center justify-center bg-emerald-400 text-white"
                                                    style={{ width: `${availPct}%` }}
                                                    title={`Available: ${bytesToGiB(available)} GiB`}
                                                >
                                                    {availPct >= 20 ? "Available" : ""}
                                                </div>
                                                <div
                                                    className="flex-1 bg-gray-200"
                                                    title={`Other: ${bytesToGiB(total - used - available)} GiB`}
                                                />
                                            </div>
                                        );
                                    })()}
                                </dt>
                                <dt className="font-medium text-gray-500">Uptime</dt>
                                <dd className="text-gray-900">
                                    {formatUptime(shadow.last_report.system_info.uptime)}
                                </dd>
                            </>
                        )}
                    </dl>
                )}
            </section>

            {/* Task States */}
            {shadow?.last_report && (
                <section className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                    <h2 className="mb-3 text-lg font-semibold text-gray-900">
                        Task States
                    </h2>
                    {(!shadow.last_report.task_states || shadow.last_report.task_states.length === 0) ? (
                        <p className="text-sm text-gray-400">No tasks</p>
                    ) : (
                        <table className="w-full text-sm">
                            <thead>
                                <tr className="border-b border-gray-200 text-left text-gray-500">
                                    <th className="pb-2 font-medium">Name</th>
                                    <th className="pb-2 font-medium">Status</th>
                                    <th className="pb-2 font-medium">Since</th>
                                </tr>
                            </thead>
                            <tbody>
                                {shadow.last_report.task_states.map((task) => (
                                    <tr key={task.name} className="border-b border-gray-100">
                                        <td className="py-2 text-gray-900">{task.name}</td>
                                        <td className="py-2">
                                            <TaskStatusBadge status={task.status} />
                                        </td>
                                        <td className="py-2 text-gray-500">
                                            <TaskStatusTimestamp status={task.status} />
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
                </section>
            )}

            {/* Assigned tags */}
            <section className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                <h2 className="mb-3 text-lg font-semibold text-gray-900">
                    Assigned Tags
                </h2>
                {edgeTags.length === 0 ? (
                    <p className="text-sm text-gray-400">No tags assigned</p>
                ) : (
                    <div className="mb-3 flex flex-wrap gap-2">
                        {edgeTags.map((tag) => {
                            const tc = getTagColor(tag.id);
                            return (
                                <span
                                    key={tag.id}
                                    className="inline-flex items-center gap-1 rounded-full px-3 py-1 text-sm font-medium"
                                    style={{ backgroundColor: tc.bg, color: tc.text }}
                                >
                                    <Link
                                        to="/tags/$tagId"
                                        params={{ tagId: tag.id }}
                                        className="hover:opacity-80"
                                    >
                                        {tagDisplayNames.get(tag.id) ?? tag.id}
                                    </Link>
                                    <button
                                        onClick={() =>
                                            setUnassignConfirm({
                                                tagId: tag.id,
                                                tagName: tagDisplayNames.get(tag.id) ?? tag.id,
                                            })
                                        }
                                        className="ml-1 opacity-60 hover:opacity-100"
                                        title="Unassign"
                                    >
                                        ×
                                    </button>
                                </span>
                            );
                        })}
                    </div>
                )}

                {/* Assign form */}
                {availableTags.length > 0 && (
                    <div className="flex items-center gap-2">
                        <select
                            value={assignTagId}
                            onChange={(e) => setAssignTagId(e.target.value)}
                            className="rounded-md border border-gray-300 px-3 py-1.5 text-sm"
                        >
                            <option value="">Select tag...</option>
                            {availableTags.map((t) => (
                                <option key={t.id} value={t.id}>
                                    {t.display_name ?? t.id}
                                </option>
                            ))}
                        </select>
                        <button
                            onClick={() => {
                                if (assignTagId) {
                                    assignMutation.mutate({
                                        edge_id: edgeId,
                                        id: assignTagId,
                                    });
                                }
                            }}
                            disabled={!assignTagId || assignMutation.isPending}
                            className="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
                        >
                            Assign
                        </button>
                    </div>
                )}
            </section>

            {/* Reported / Desired State */}
            {shadow?.last_report && (
                <section className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                    <h2 className="mb-3 text-lg font-semibold text-gray-900">
                        State Metadata
                    </h2>
                    <div className="space-y-4">
                        <StateMetadataView
                            label="Reported State (current_state)"
                            metadata={shadow.last_report.current_state}
                            tagDisplayNames={tagDisplayNames}
                        />
                        {shadow.last_report.desired_state && (
                            <StateMetadataView
                                label="Reported State (desired_state)"
                                metadata={shadow.last_report.desired_state}
                                tagDisplayNames={tagDisplayNames}
                            />
                        )}
                        {shadow.desired_state?.metadata && (
                            <StateMetadataView
                                label="Desired State (metadata)"
                                metadata={shadow.desired_state.metadata}
                                tagDisplayNames={tagDisplayNames}
                            />
                        )}
                    </div>
                </section>
            )}

            {/* Unassign confirmation dialog */}
            {unassignConfirm && (
                <ConfirmDialog
                    onConfirm={() => {
                        unassignMutation.mutate({
                            edge_id: edgeId,
                            id: unassignConfirm.tagId,
                        });
                        setUnassignConfirm(null);
                    }}
                    onCancel={() => setUnassignConfirm(null)}
                >
                    <p>
                        Unassign tag{" "}
                        <span className="font-medium">{unassignConfirm.tagName}</span>{" "}
                        from this edge?
                    </p>
                    <p className="mt-1 text-xs text-gray-500">
                        The edge's merged configuration will be recalculated without this tag's patch.
                    </p>
                </ConfirmDialog>
            )}

            {/* Merged config preview */}
            <section className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                <div className="mb-3 flex items-center justify-between">
                    <h2 className="text-lg font-semibold text-gray-900">
                        Merged Configuration Preview
                    </h2>
                    <div className="flex items-center gap-2">
                        <div className="inline-flex rounded-md border border-gray-300 text-sm">
                            {(["json", "toml"] as const).map((fmt) => (
                                <button
                                    key={fmt}
                                    onClick={() => setPreviewFormat(fmt)}
                                    className={`px-3 py-1 font-medium transition-colors ${previewFormat === fmt
                                        ? "bg-indigo-600 text-white"
                                        : "text-gray-600 hover:bg-gray-100"
                                        } ${fmt === "json" ? "rounded-l-md" : "rounded-r-md"}`}
                                >
                                    {fmt.toUpperCase()}
                                </button>
                            ))}
                        </div>
                        <button
                            onClick={handleCopy}
                            disabled={!previewText}
                            className="rounded-md border border-gray-300 px-3 py-1 text-sm font-medium text-gray-600 transition-colors hover:bg-gray-100 disabled:opacity-50"
                        >
                            {copied ? "Copied!" : "Copy"}
                        </button>
                    </div>
                </div>
                {previewQuery.isLoading ? (
                    <p className="text-sm text-gray-500">Loading...</p>
                ) : previewQuery.error ? (
                    <p className="text-sm text-red-600">
                        {String(previewQuery.error)}
                    </p>
                ) : (
                    <pre className="max-h-96 overflow-auto rounded-md bg-gray-50 p-3 text-xs">
                        {unescapeNewlines(previewText)}
                    </pre>
                )}
            </section>
        </div>
    );
}

function StateMetadataView({
    label,
    metadata,
    tagDisplayNames,
}: {
    label: string;
    metadata: StateMetadata;
    tagDisplayNames: Map<string, string>;
}) {
    return (
        <div>
            <h3 className="text-sm font-semibold text-gray-700">{label}</h3>
            <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                <dt className="font-medium text-gray-500">Schema Version</dt>
                <dd className="text-gray-900">{metadata.schema_version}</dd>
                <dt className="font-medium text-gray-500">Epoch (Tags)</dt>
                <dd>
                    {metadata.epoch.length === 0 ? (
                        <span className="text-gray-400">—</span>
                    ) : (
                        <div className="flex flex-wrap gap-1">
                            {metadata.epoch.map((t) => {
                                const tc = getTagColor(t.id);
                                const name = tagDisplayNames.get(t.id) ?? t.display_name ?? t.id;
                                return (
                                    <Link
                                        key={t.id}
                                        to="/tags/$tagId"
                                        params={{ tagId: t.id }}
                                        className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium hover:opacity-80"
                                        style={{ backgroundColor: tc.bg, color: tc.text }}
                                        title={`ID: ${t.id}\nEpoch: ${t.epoch}`}
                                    >
                                        {name}
                                        <span className="opacity-60">e{t.epoch}</span>
                                    </Link>
                                );
                            })}
                        </div>
                    )}
                </dd>
            </dl>
        </div>
    );
}

function getTaskStatusLabel(status: TaskStatus): string {
    if (status === "Created") return "Created";
    if (status === "Discarded") return "Discarded";
    if (typeof status === "object" && "Active" in status) return "Active";
    if (typeof status === "object" && "Inactive" in status) return "Inactive";
    return "Unknown";
}

const taskStatusStyles: Record<string, string> = {
    Created: "bg-gray-100 text-gray-700",
    Active: "bg-green-100 text-green-700",
    Inactive: "bg-yellow-100 text-yellow-700",
    Discarded: "bg-red-100 text-red-700",
    Unknown: "bg-gray-100 text-gray-500",
};

function TaskStatusBadge({ status }: { status: TaskStatus }) {
    const label = getTaskStatusLabel(status);
    const style = taskStatusStyles[label] ?? taskStatusStyles.Unknown;
    return (
        <span className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${style}`}>
            {label}
        </span>
    );
}

function TaskStatusTimestamp({ status }: { status: TaskStatus }) {
    if (typeof status === "object" && "Active" in status) {
        return <>{formatTimestamp(status.Active)}</>;
    }
    if (typeof status === "object" && "Inactive" in status) {
        return <>{formatTimestamp(status.Inactive)}</>;
    }
    return <span className="text-gray-300">—</span>;
}
