// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
    useReactTable,
    getCoreRowModel,
    getFilteredRowModel,
    getSortedRowModel,
    flexRender,
    type ColumnDef,
    type SortingState,
    type RowSelectionState,
} from "@tanstack/react-table";
import { edges, shadows, tags as tagsApi } from "../../api/client";
import type {
    AllTagAssignments,
    AssignedTag,
    BatchCommandResponse,
    CommandKind,
    EdgeShadow,
    EdgeStatus,
} from "../../api/types";
import { cn, formatTimestamp, getTagColor } from "../../lib/utils";
import { Link } from "@tanstack/react-router";
import { ChevronDown, ChevronUp, ChevronsUpDown } from "lucide-react";
import { toast } from "../../stores/toastStore";

// ---------------------------------------------------------------------------
// Row type enriched with tag info
// ---------------------------------------------------------------------------

interface EdgeRow extends EdgeShadow {
    tags: AssignedTag[];
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PAGE_SIZE = 20;

const ALL_STATUSES: (EdgeStatus | "Unknown")[] = [
    "Up", "Starting", "Stopping", "Down", "Error", "Suspended", "Unknown",
];

const COMMANDS: { kind: CommandKind; label: string }[] = [
    { kind: "start", label: "Start" },
    { kind: "shutdown", label: "Shutdown" },
    { kind: "restart", label: "Restart" },
    { kind: "suspend", label: "Suspend" },
];

// ---------------------------------------------------------------------------
// Small components
// ---------------------------------------------------------------------------

function StatusBadge({ status }: { status: string }) {
    const color =
        {
            Up: "bg-green-100 text-green-700",
            Down: "bg-gray-100 text-gray-600",
            Error: "bg-red-100 text-red-700",
            Starting: "bg-yellow-100 text-yellow-700",
            Stopping: "bg-orange-100 text-orange-700",
            Suspended: "bg-purple-100 text-purple-700",
        }[status] ?? "bg-gray-100 text-gray-600";

    return (
        <span
            className={cn(
                "inline-block rounded-full px-2 py-0.5 text-xs font-medium",
                color,
            )}
        >
            {status}
        </span>
    );
}

function SortIcon({ isSorted }: { isSorted: false | "asc" | "desc" }) {
    if (isSorted === "asc") return <ChevronUp className="ml-1 inline h-3 w-3" />;
    if (isSorted === "desc") return <ChevronDown className="ml-1 inline h-3 w-3" />;
    return <ChevronsUpDown className="ml-1 inline h-3 w-3 text-gray-300" />;
}

// ---------------------------------------------------------------------------
// Main page
// ---------------------------------------------------------------------------

export function DashboardPage() {
    // ---- Data fetching ----
    const {
        data: shadowData,
        isLoading,
        error,
    } = useQuery({
        queryKey: ["shadows"],
        queryFn: shadows.list,
        refetchInterval: 10000,
    });

    const { data: tagData } = useQuery({
        queryKey: ["all-edge-tags"],
        queryFn: edges.getAllTags,
        refetchInterval: 30000,
    });

    const { data: allTagsData } = useQuery({
        queryKey: ["tags"],
        queryFn: tagsApi.list,
    });

    const tagDisplayNames = useMemo(() => {
        const map = new Map<string, string>();
        for (const t of allTagsData ?? []) {
            map.set(t.id, t.display_name ?? t.id);
        }
        return map;
    }, [allTagsData]);

    const allTags: AllTagAssignments = tagData ?? {};

    // ---- Build row data ----
    const rows: EdgeRow[] = useMemo(() => {
        if (!shadowData) return [];
        return Object.entries(shadowData).map(([id, s]) => ({
            ...s,
            edge_id: id,
            tags: allTags[id] ?? [],
        }));
    }, [shadowData, allTags]);

    // ---- Filter state ----
    const [searchText, setSearchText] = useState("");
    const [tagFilter, setTagFilter] = useState("");
    const [statusFilter, setStatusFilter] = useState("Up");

    // Collect unique tag IDs for the filter dropdown
    const allTagIds = useMemo(() => {
        const set = new Set<string>();
        for (const tags of Object.values(allTags)) {
            for (const t of tags) set.add(t.id);
        }
        return Array.from(set).sort();
    }, [allTags]);

    // ---- Table state ----
    const [sorting, setSorting] = useState<SortingState>([]);
    const [rowSelection, setRowSelection] = useState<RowSelectionState>({});
    const [selectAllFiltered, setSelectAllFiltered] = useState(false);

    // ---- Column definitions ----
    const columns = useMemo<ColumnDef<EdgeRow, unknown>[]>(
        () => [
            {
                id: "select",
                header: ({ table }) => (
                    <input
                        type="checkbox"
                        className="rounded"
                        checked={table.getIsAllPageRowsSelected()}
                        ref={(el) => {
                            if (el)
                                el.indeterminate =
                                    table.getIsSomePageRowsSelected() &&
                                    !table.getIsAllPageRowsSelected();
                        }}
                        onChange={(e) => {
                            table.toggleAllPageRowsSelected(e.target.checked);
                            if (!e.target.checked) setSelectAllFiltered(false);
                        }}
                    />
                ),
                cell: ({ row }) => (
                    <input
                        type="checkbox"
                        className="rounded"
                        checked={selectAllFiltered || row.getIsSelected()}
                        onChange={row.getToggleSelectedHandler()}
                    />
                ),
                enableSorting: false,
                size: 40,
            },
            {
                accessorKey: "edge_id",
                header: "Edge ID",
                cell: ({ row }) => (
                    <Link
                        to="/edges/$edgeId"
                        params={{ edgeId: row.original.edge_id }}
                        className="font-medium text-indigo-600 hover:underline"
                    >
                        {row.original.edge_id}
                    </Link>
                ),
            },
            {
                id: "status",
                accessorFn: (row) => row.last_report?.status ?? "Unknown",
                header: "Status",
                cell: ({ getValue }) => (
                    <StatusBadge status={getValue() as string} />
                ),
            },
            {
                id: "place",
                accessorFn: (row) => row.last_report?.place_id ?? "unknown",
                header: "Place",
                cell: ({ getValue }) => (
                    <span className="text-sm text-gray-700">{getValue() as string}</span>
                ),
            },
            {
                id: "tags",
                accessorFn: (row) =>
                    row.tags.map((t) => t.id).join(", "),
                header: "Tags",
                cell: ({ row }) => {
                    const t = row.original.tags;
                    if (t.length === 0)
                        return (
                            <span className="text-gray-400 text-xs">—</span>
                        );
                    const show = t.slice(0, 3);
                    const rest = t.length - 3;
                    return (
                        <div className="flex flex-wrap gap-1">
                            {show.map((tag) => {
                                const tc = getTagColor(tag.id);
                                return (
                                    <span
                                        key={tag.id}
                                        className="inline-block rounded px-1.5 py-0.5 text-xs"
                                        style={{ backgroundColor: tc.bg, color: tc.text }}
                                    >
                                        {tagDisplayNames.get(tag.id) ?? tag.id}
                                    </span>
                                );
                            })}
                            {rest > 0 && (
                                <span className="text-xs text-gray-400">
                                    +{rest}
                                </span>
                            )}
                        </div>
                    );
                },
                enableSorting: false,
            },
            {
                id: "last_seen",
                accessorFn: (row) => row.last_seen ?? 0,
                header: "Last Seen",
                cell: ({ row }) =>
                    row.original.last_seen != null
                        ? formatTimestamp(row.original.last_seen)
                        : "—",
            },
        ],
        [selectAllFiltered, tagDisplayNames],
    );

    // ---- Table instance ----
    // Combine searchText + tagFilter into a single globalFilter value so
    // that changes to either trigger re-filtering.
    const combinedFilter = `${searchText}\0${tagFilter}\0${statusFilter}`;

    const table = useReactTable({
        data: rows,
        columns,
        state: { sorting, rowSelection, globalFilter: combinedFilter },
        onSortingChange: setSorting,
        onRowSelectionChange: (updater) => {
            setRowSelection(updater);
            setSelectAllFiltered(false);
        },
        globalFilterFn: (row, _columnId, filterValue) => {
            const parts = (filterValue as string).split("\0");
            const search = parts[0]?.toLowerCase() ?? "";
            const tag = parts[1] ?? "";
            const status = parts[2] ?? "";
            if (search && !row.original.edge_id.toLowerCase().includes(search))
                return false;
            if (tag === "__none__") {
                if (row.original.tags.length > 0) return false;
            } else if (tag && !row.original.tags.some((t) => t.id === tag))
                return false;
            if (status && (row.original.last_report?.status ?? "Unknown") !== status)
                return false;
            return true;
        },
        getCoreRowModel: getCoreRowModel(),
        getFilteredRowModel: getFilteredRowModel(),
        getSortedRowModel: getSortedRowModel(),
        enableRowSelection: true,
    });

    // ---- Infinite scroll ----
    const allFilteredRows = table.getFilteredRowModel().rows;
    const sortedRows = table.getSortedRowModel().rows;
    const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);

    // Reset visible count when filters or sorting change
    useEffect(() => {
        setVisibleCount(PAGE_SIZE);
    }, [combinedFilter, sorting]);

    const hasMore = visibleCount < sortedRows.length;
    const sentinelRef = useRef<HTMLDivElement>(null);
    const handleIntersect = useCallback(
        (entries: IntersectionObserverEntry[]) => {
            if (entries[0]?.isIntersecting && hasMore) {
                setVisibleCount((c) => Math.min(c + PAGE_SIZE, sortedRows.length));
            }
        },
        [hasMore, sortedRows.length],
    );

    useEffect(() => {
        const el = sentinelRef.current;
        if (!el) return;
        const observer = new IntersectionObserver(handleIntersect, {
            rootMargin: "200px",
        });
        observer.observe(el);
        return () => observer.disconnect();
    }, [handleIntersect]);

    // ---- Selection helpers ----
    const selectedIds: string[] = useMemo(() => {
        if (selectAllFiltered) {
            return table
                .getFilteredRowModel()
                .rows.map((r) => r.original.edge_id);
        }
        return Object.keys(rowSelection)
            .filter((k) => rowSelection[k])
            .map((idx) => rows[Number(idx)]?.edge_id)
            .filter(Boolean);
    }, [selectAllFiltered, rowSelection, rows, table]);

    const selectedCount = selectedIds.length;
    const filteredCount = allFilteredRows.length;

    // ---- Summary stats ----
    const totalCount = rows.length;
    const upCount = rows.filter(
        (s) => s.last_report?.status === "Up",
    ).length;

    // ---- Batch command ----
    const queryClient = useQueryClient();
    const [actionOpen, setActionOpen] = useState(false);
    const [assignTagSubmenuOpen, setAssignTagSubmenuOpen] = useState(false);
    const [unassignTagSubmenuOpen, setUnassignTagSubmenuOpen] = useState(false);
    const [confirmDialog, setConfirmDialog] = useState<
        | { type: "command"; kind: CommandKind; count: number }
        | { type: "assign-tag"; tagId: string; tagName: string; edgeIds: string[] }
        | { type: "unassign-tag"; tagId: string; tagName: string; edgeIds: string[] }
        | null
    >(null);

    const batchMutation = useMutation({
        mutationFn: (body: { edge_ids: string[]; kind: CommandKind }) =>
            edges.batchCommand(body),
        onSuccess: (result: BatchCommandResponse) => {
            queryClient.invalidateQueries({ queryKey: ["shadows"] });
            setRowSelection({});
            setSelectAllFiltered(false);
            if (result.failed.length > 0) {
                toast.warning(
                    `${result.succeeded.length} succeeded, ${result.failed.length} failed:\n` +
                    result.failed
                        .map((f) => `  ${f.edge_id}: ${f.error}`)
                        .join("\n"),
                );
            }
        },
    });

    function handleAction(kind: CommandKind) {
        setActionOpen(false);
        setAssignTagSubmenuOpen(false);
        setUnassignTagSubmenuOpen(false);
        setConfirmDialog({ type: "command", kind, count: selectedCount });
    }

    function handleAssignTagSelect(tagId: string) {
        setActionOpen(false);
        setAssignTagSubmenuOpen(false);
        setUnassignTagSubmenuOpen(false);
        setConfirmDialog({
            type: "assign-tag",
            tagId,
            tagName: tagDisplayNames.get(tagId) ?? tagId,
            edgeIds: selectedIds,
        });
    }

    function handleUnassignTagSelect(tagId: string) {
        setActionOpen(false);
        setAssignTagSubmenuOpen(false);
        setUnassignTagSubmenuOpen(false);
        setConfirmDialog({
            type: "unassign-tag",
            tagId,
            tagName: tagDisplayNames.get(tagId) ?? tagId,
            edgeIds: selectedIds,
        });
    }

    const assignTagMutation = useMutation({
        mutationFn: async ({ tagId, edgeIds }: { tagId: string; edgeIds: string[] }) => {
            const results = await Promise.allSettled(
                edgeIds.map((edge_id) => tagsApi.assign({ edge_id, id: tagId }))
            );
            const failed = results
                .map((r, i) => ({ r, edge_id: edgeIds[i] }))
                .filter(({ r }) => r.status === "rejected");
            return { total: edgeIds.length, failed };
        },
        onSuccess: ({ total, failed }) => {
            queryClient.invalidateQueries({ queryKey: ["all-edge-tags"] });
            setRowSelection({});
            setSelectAllFiltered(false);
            if (failed.length > 0) {
                alert(
                    `${total - failed.length}/${total} succeeded.\nFailed edges:\n` +
                    failed.map(({ edge_id }) => `  ${edge_id}`).join("\n")
                );
            }
        },
    });

    const unassignTagMutation = useMutation({
        mutationFn: async ({ tagId, edgeIds }: { tagId: string; edgeIds: string[] }) => {
            const results = await Promise.allSettled(
                edgeIds.map((edge_id) => tagsApi.unassign({ edge_id, id: tagId }))
            );
            const failed = results
                .map((r, i) => ({ r, edge_id: edgeIds[i] }))
                .filter(({ r }) => r.status === "rejected");
            return { total: edgeIds.length, failed };
        },
        onSuccess: ({ total, failed }) => {
            queryClient.invalidateQueries({ queryKey: ["all-edge-tags"] });
            setRowSelection({});
            setSelectAllFiltered(false);
            if (failed.length > 0) {
                alert(
                    `${total - failed.length}/${total} succeeded.\nFailed edges:\n` +
                    failed.map(({ edge_id }) => `  ${edge_id}`).join("\n")
                );
            }
        },
    });

    function handleConfirm() {
        if (!confirmDialog) return;
        if (confirmDialog.type === "command") {
            batchMutation.mutate({
                edge_ids: selectedIds,
                kind: confirmDialog.kind,
            });
        } else if (confirmDialog.type === "assign-tag") {
            assignTagMutation.mutate({
                tagId: confirmDialog.tagId,
                edgeIds: confirmDialog.edgeIds,
            });
        } else {
            unassignTagMutation.mutate({
                tagId: confirmDialog.tagId,
                edgeIds: confirmDialog.edgeIds,
            });
        }
        setConfirmDialog(null);
    }

    // ---- Render ----
    return (
        <div className="space-y-6">
            <h1 className="text-2xl font-bold text-gray-900">Dashboard</h1>

            {/* Summary cards */}
            <div className="grid grid-cols-3 gap-4">
                <div className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                    <p className="text-sm text-gray-500">Total Edges</p>
                    <p className="text-3xl font-bold text-gray-900">
                        {totalCount}
                    </p>
                </div>
                <div className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                    <p className="text-sm text-gray-500">Online</p>
                    <p className="text-3xl font-bold text-green-600">
                        {upCount}
                    </p>
                </div>
                <div className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
                    <p className="text-sm text-gray-500">Offline</p>
                    <p className="text-3xl font-bold text-gray-400">
                        {totalCount - upCount}
                    </p>
                </div>
            </div>

            {/* Toolbar */}
            <div className="flex flex-wrap items-center gap-3">
                {/* Search */}
                <input
                    type="text"
                    placeholder="Search Edge ID..."
                    value={searchText}
                    onChange={(e) => setSearchText(e.target.value)}
                    className="rounded border border-gray-300 px-3 py-1.5 text-sm focus:border-indigo-400 focus:outline-none focus:ring-1 focus:ring-indigo-400"
                />

                {/* Tag filter */}
                <select
                    value={tagFilter}
                    onChange={(e) => setTagFilter(e.target.value)}
                    className="rounded border border-gray-300 px-3 py-1.5 text-sm focus:border-indigo-400 focus:outline-none focus:ring-1 focus:ring-indigo-400"
                >
                    <option value="">All Tags</option>
                    <option value="__none__">— (no tags)</option>
                    {allTagIds.map((id) => (
                        <option key={id} value={id}>
                            {tagDisplayNames.get(id) ?? id}
                        </option>
                    ))}
                </select>

                {/* Status filter */}
                <select
                    value={statusFilter}
                    onChange={(e) => setStatusFilter(e.target.value)}
                    className="rounded border border-gray-300 px-3 py-1.5 text-sm focus:border-indigo-400 focus:outline-none focus:ring-1 focus:ring-indigo-400"
                >
                    <option value="">All Statuses</option>
                    {ALL_STATUSES.map((s) => (
                        <option key={s} value={s}>
                            {s}
                        </option>
                    ))}
                </select>

                <div className="flex-1" />

                {/* Selected count */}
                {selectedCount > 0 && (
                    <span className="text-sm text-gray-600">
                        {selectedCount} selected
                    </span>
                )}

                {/* Action dropdown */}
                <div className="relative">
                    <button
                        disabled={selectedCount === 0 || batchMutation.isPending || assignTagMutation.isPending || unassignTagMutation.isPending}
                        onClick={() => {
                            setActionOpen((v) => !v);
                            setAssignTagSubmenuOpen(false);
                            setUnassignTagSubmenuOpen(false);
                        }}
                        className={cn(
                            "inline-flex items-center gap-1 rounded border px-3 py-1.5 text-sm font-medium transition",
                            selectedCount > 0
                                ? "border-indigo-300 bg-indigo-50 text-indigo-700 hover:bg-indigo-100"
                                : "cursor-not-allowed border-gray-200 bg-gray-50 text-gray-400",
                        )}
                    >
                        Action
                        <ChevronDown className="h-3 w-3" />
                    </button>
                    {actionOpen && selectedCount > 0 && (
                        <div className="absolute right-0 z-10 mt-1 w-40 rounded border border-gray-200 bg-white py-1 shadow-lg">
                            {COMMANDS.map(({ kind, label }) => (
                                <button
                                    key={kind}
                                    onClick={() => handleAction(kind)}
                                    className="block w-full px-3 py-1.5 text-left text-sm hover:bg-gray-100"
                                >
                                    {label}
                                </button>
                            ))}
                            {/* Assign Tag submenu */}
                            <div className="relative">
                                <button
                                    onClick={() => {
                                        setAssignTagSubmenuOpen((v) => !v);
                                        setUnassignTagSubmenuOpen(false);
                                    }}
                                    className="flex w-full items-center justify-between px-3 py-1.5 text-left text-sm hover:bg-gray-100"
                                >
                                    Assign Tag
                                    <ChevronDown className="h-3 w-3 -rotate-90" />
                                </button>
                                {assignTagSubmenuOpen && (
                                    <div className="absolute right-full top-0 mr-1 max-h-60 w-44 overflow-y-auto rounded border border-gray-200 bg-white py-1 shadow-lg">
                                        {(allTagsData ?? []).length === 0 ? (
                                            <span className="block px-3 py-1.5 text-xs text-gray-400">No tags defined</span>
                                        ) : (
                                            (allTagsData ?? []).map((t) => {
                                                const tc = getTagColor(t.id);
                                                return (
                                                    <button
                                                        key={t.id}
                                                        onClick={() => handleAssignTagSelect(t.id)}
                                                        className="block w-full px-3 py-1.5 text-left text-sm hover:bg-gray-50"
                                                    >
                                                        <span
                                                            className="inline-block rounded px-1.5 py-0.5 text-xs"
                                                            style={{ backgroundColor: tc.bg, color: tc.text }}
                                                        >
                                                            {t.display_name ?? t.id}
                                                        </span>
                                                    </button>
                                                );
                                            })
                                        )}
                                    </div>
                                )}
                            </div>
                            {/* Unassign Tag submenu */}
                            <div className="relative">
                                <button
                                    onClick={() => {
                                        setUnassignTagSubmenuOpen((v) => !v);
                                        setAssignTagSubmenuOpen(false);
                                    }}
                                    className="flex w-full items-center justify-between px-3 py-1.5 text-left text-sm hover:bg-gray-100"
                                >
                                    Unassign Tag
                                    <ChevronDown className="h-3 w-3 -rotate-90" />
                                </button>
                                {unassignTagSubmenuOpen && (
                                    <div className="absolute right-full top-0 mr-1 max-h-60 w-44 overflow-y-auto rounded border border-gray-200 bg-white py-1 shadow-lg">
                                        {(allTagsData ?? []).length === 0 ? (
                                            <span className="block px-3 py-1.5 text-xs text-gray-400">No tags defined</span>
                                        ) : (
                                            (allTagsData ?? []).map((t) => {
                                                const tc = getTagColor(t.id);
                                                return (
                                                    <button
                                                        key={t.id}
                                                        onClick={() => handleUnassignTagSelect(t.id)}
                                                        className="block w-full px-3 py-1.5 text-left text-sm hover:bg-gray-50"
                                                    >
                                                        <span
                                                            className="inline-block rounded px-1.5 py-0.5 text-xs"
                                                            style={{ backgroundColor: tc.bg, color: tc.text }}
                                                        >
                                                            {t.display_name ?? t.id}
                                                        </span>
                                                    </button>
                                                );
                                            })
                                        )}
                                    </div>
                                )}
                            </div>
                        </div>
                    )}
                </div>
            </div>

            {/* Edge table */}
            {isLoading ? (
                <p className="text-sm text-gray-500">Loading...</p>
            ) : error ? (
                <p className="text-sm text-red-600">
                    Failed to load edges: {String(error)}
                </p>
            ) : (
                <>
                    <div className="overflow-hidden rounded-lg border border-gray-200">
                        <table className="min-w-full divide-y divide-gray-200">
                            <thead className="bg-gray-50">
                                {table.getHeaderGroups().map((hg) => (
                                    <tr key={hg.id}>
                                        {hg.headers.map((header) => (
                                            <th
                                                key={header.id}
                                                className={cn(
                                                    "px-4 py-3 text-left text-xs font-medium uppercase text-gray-500",
                                                    header.column.getCanSort() &&
                                                    "cursor-pointer select-none",
                                                )}
                                                style={{
                                                    width:
                                                        header.column.columnDef
                                                            .size ?? undefined,
                                                }}
                                                onClick={header.column.getToggleSortingHandler()}
                                            >
                                                {header.isPlaceholder
                                                    ? null
                                                    : flexRender(
                                                        header.column
                                                            .columnDef.header,
                                                        header.getContext(),
                                                    )}
                                                {header.column.getCanSort() && (
                                                    <SortIcon
                                                        isSorted={header.column.getIsSorted()}
                                                    />
                                                )}
                                            </th>
                                        ))}
                                    </tr>
                                ))}
                            </thead>
                            <tbody className="divide-y divide-gray-200 bg-white">
                                {sortedRows.length === 0 ? (
                                    <tr>
                                        <td
                                            colSpan={columns.length}
                                            className="px-4 py-8 text-center text-sm text-gray-400"
                                        >
                                            No edges registered yet
                                        </td>
                                    </tr>
                                ) : (
                                    sortedRows.slice(0, visibleCount).map((row) => (
                                        <tr
                                            key={row.id}
                                            className={cn(
                                                "hover:bg-gray-50",
                                                (selectAllFiltered ||
                                                    row.getIsSelected()) &&
                                                "bg-indigo-50/40",
                                            )}
                                        >
                                            {row
                                                .getVisibleCells()
                                                .map((cell) => (
                                                    <td
                                                        key={cell.id}
                                                        className="px-4 py-3 text-sm"
                                                    >
                                                        {flexRender(
                                                            cell.column
                                                                .columnDef.cell,
                                                            cell.getContext(),
                                                        )}
                                                    </td>
                                                ))}
                                        </tr>
                                    ))
                                )}
                            </tbody>
                        </table>
                    </div>

                    {/* Select-all banner */}
                    {selectAllFiltered && (
                        <div className="text-center text-sm text-indigo-600 font-medium">
                            All {filteredCount} filtered edges are selected.{" "}
                            <button
                                className="text-gray-500 hover:underline"
                                onClick={() => {
                                    setSelectAllFiltered(false);
                                    setRowSelection({});
                                }}
                            >
                                Clear selection
                            </button>
                        </div>
                    )}

                    {/* Infinite scroll sentinel */}
                    <div ref={sentinelRef} />
                    <p className="text-center text-xs text-gray-400">
                        Showing {Math.min(visibleCount, sortedRows.length)} of{" "}
                        {filteredCount} edge{filteredCount !== 1 ? "s" : ""}
                        {hasMore && " — scroll for more"}
                    </p>
                </>
            )}

            {/* Confirmation dialog */}
            {confirmDialog && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30">
                    <div className="w-full max-w-sm rounded-lg bg-white p-6 shadow-xl">
                        <h2 className="text-lg font-semibold text-gray-900">
                            Confirm Action
                        </h2>
                        {confirmDialog.type === "command" ? (
                            <p className="mt-2 text-sm text-gray-600">
                                Send{" "}
                                <span className="font-medium">
                                    {confirmDialog.kind}
                                </span>{" "}
                                command to{" "}
                                <span className="font-medium">
                                    {confirmDialog.count}
                                </span>{" "}
                                edge{confirmDialog.count !== 1 ? "s" : ""}?
                            </p>
                        ) : confirmDialog.type === "assign-tag" ? (
                            <p className="mt-2 text-sm text-gray-600">
                                Assign tag{" "}
                                <span className="font-medium">
                                    {confirmDialog.tagName}
                                </span>{" "}
                                to{" "}
                                <span className="font-medium">
                                    {confirmDialog.edgeIds.length}
                                </span>{" "}
                                edge{confirmDialog.edgeIds.length !== 1 ? "s" : ""}?
                                <br />
                                <span className="mt-1 block text-xs text-gray-500">
                                    Already-assigned edges will be skipped automatically.
                                </span>
                            </p>
                        ) : (
                            <p className="mt-2 text-sm text-gray-600">
                                Unassign tag{" "}
                                <span className="font-medium">
                                    {confirmDialog.tagName}
                                </span>{" "}
                                from{" "}
                                <span className="font-medium">
                                    {confirmDialog.edgeIds.length}
                                </span>{" "}
                                edge{confirmDialog.edgeIds.length !== 1 ? "s" : ""}?
                                <br />
                                <span className="mt-1 block text-xs text-gray-500">
                                    The merged configuration of affected edges will be recalculated without this tag's patch.
                                </span>
                            </p>
                        )}
                        <div className="mt-4 flex justify-end gap-2">
                            <button
                                onClick={() => setConfirmDialog(null)}
                                className="rounded border border-gray-300 px-4 py-1.5 text-sm hover:bg-gray-50"
                            >
                                No
                            </button>
                            <button
                                onClick={handleConfirm}
                                className="rounded bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-700"
                            >
                                Yes
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
