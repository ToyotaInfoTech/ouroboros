// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useQuery } from "@tanstack/react-query";
import { shadows, tags as tagsApi, edges as edgesApi } from "../../api/client";
import type { EdgeShadow, EdgeStatus } from "../../api/types";
import { Link } from "@tanstack/react-router";
import { formatTimestamp } from "../../lib/utils";
import { useCallback, useEffect, useRef, useState } from "react";

const PAGE_SIZE = 20;

const ALL_STATUSES: (EdgeStatus | "Unknown")[] = [
    "Up", "Starting", "Stopping", "Down", "Error", "Suspended", "Unknown",
];

export function EdgesPage() {
    const [selectedTag, setSelectedTag] = useState<string>("");
    const [selectedStatus, setSelectedStatus] = useState<string>("");

    const { data, isLoading, error } = useQuery({
        queryKey: ["shadows"],
        queryFn: shadows.list,
        refetchInterval: 10000,
    });

    const { data: tagList } = useQuery({
        queryKey: ["tags"],
        queryFn: tagsApi.list,
    });

    const { data: allTagAssignments } = useQuery({
        queryKey: ["edges", "tags"],
        queryFn: edgesApi.getAllTags,
    });

    const shadowList: EdgeShadow[] = data
        ? Object.entries(data).map(([id, s]) => ({ ...s, edge_id: id }))
        : [];

    const filteredList = shadowList.filter((shadow) => {
        if (selectedTag) {
            const assigned = allTagAssignments?.[shadow.edge_id] ?? [];
            if (!assigned.some((t) => t.id === selectedTag)) return false;
        }
        if (selectedStatus) {
            const status = shadow.last_report?.status ?? "Unknown";
            if (status !== selectedStatus) return false;
        }
        return true;
    });

    // ---- Infinite scroll ----
    const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);

    useEffect(() => {
        // eslint-disable-next-line react-hooks/set-state-in-effect
        setVisibleCount(PAGE_SIZE);
    }, [selectedTag, selectedStatus]);

    const hasMore = visibleCount < filteredList.length;
    const sentinelRef = useRef<HTMLDivElement>(null);
    const handleIntersect = useCallback(
        (entries: IntersectionObserverEntry[]) => {
            if (entries[0]?.isIntersecting && hasMore) {
                setVisibleCount((c) => Math.min(c + PAGE_SIZE, filteredList.length));
            }
        },
        [hasMore, filteredList.length],
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

    return (
        <div className="space-y-6">
            <h1 className="text-2xl font-bold text-gray-900">Edges</h1>

            <div className="flex flex-wrap gap-3">
                <select
                    value={selectedTag}
                    onChange={(e) => setSelectedTag(e.target.value)}
                    className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-700 shadow-sm focus:outline-none focus:ring-2 focus:ring-indigo-500"
                >
                    <option value="">All Tags</option>
                    {(tagList ?? []).map((tag) => (
                        <option key={tag.id} value={tag.id}>
                            {tag.display_name ?? tag.id}
                        </option>
                    ))}
                </select>

                <select
                    value={selectedStatus}
                    onChange={(e) => setSelectedStatus(e.target.value)}
                    className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-700 shadow-sm focus:outline-none focus:ring-2 focus:ring-indigo-500"
                >
                    <option value="">All Statuses</option>
                    {ALL_STATUSES.map((s) => (
                        <option key={s} value={s}>
                            {s}
                        </option>
                    ))}
                </select>
            </div>

            {isLoading ? (
                <p className="text-sm text-gray-500">Loading...</p>
            ) : error ? (
                <p className="text-sm text-red-600">
                    Failed to load edges: {String(error)}
                </p>
            ) : (
                <>
                    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                        {filteredList.length === 0 ? (
                            <p className="text-sm text-gray-400">No edges registered yet</p>
                        ) : (
                            filteredList.slice(0, visibleCount).map((shadow) => (
                                <Link
                                    key={shadow.edge_id}
                                    to="/edges/$edgeId"
                                    params={{ edgeId: shadow.edge_id }}
                                    className="block rounded-lg border border-gray-200 bg-white p-4 shadow-sm transition-shadow hover:shadow-md"
                                >
                                    <h3 className="text-sm font-semibold text-gray-900">
                                        {shadow.edge_id}
                                    </h3>
                                    <p className="mt-1 text-xs text-gray-500">
                                        Status: {shadow.last_report?.status ?? "Unknown"}
                                    </p>
                                    <p className="mt-0.5 text-xs text-gray-500">
                                        Place: {shadow.last_report?.place_id ?? "unknown"}
                                    </p>
                                    {shadow.last_seen != null && (
                                        <p className="mt-0.5 text-xs text-gray-400">
                                            Last seen: {formatTimestamp(shadow.last_seen)}
                                        </p>
                                    )}
                                </Link>
                            ))
                        )}
                    </div>
                    <div ref={sentinelRef} />
                    {filteredList.length > 0 && (
                        <p className="text-center text-xs text-gray-400">
                            Showing {Math.min(visibleCount, filteredList.length)} of{" "}
                            {filteredList.length} edge{filteredList.length !== 1 ? "s" : ""}
                            {hasMore && " \u2014 scroll for more"}
                        </p>
                    )}
                </>
            )}
        </div>
    );
}
