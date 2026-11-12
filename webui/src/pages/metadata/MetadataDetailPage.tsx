// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "@tanstack/react-router";
import { metadata as metadataApi } from "../../api/client";
import type { LocationFix, CameraFrameRef, ActiveBehavior } from "../../api/types";
import MapGL, { Source, Layer } from "react-map-gl/maplibre";
import type { MapRef, MapMouseEvent } from "react-map-gl/maplibre";
import { LngLatBounds } from "maplibre-gl";
import { useState, useMemo, useRef, useCallback, useEffect } from "react";
import { MAP_STYLE } from "../../lib/mapUtils";

const CAMERA_ORDER = [
    "front",
    "front_left",
    "front_right",
    "rear_left",
    "rear_right",
    "rear",
] as const;

function formatDateTime(iso: string): string {
    try {
        return new Date(iso).toLocaleString();
    } catch {
        return iso;
    }
}

function formatTime(iso: string): string {
    try {
        return new Date(iso).toLocaleTimeString();
    } catch {
        return iso;
    }
}

/** Return active behaviors whose time range covers the given timestamp. */
function getActiveBehaviorsAt(ts: string, behaviors: ActiveBehavior[]): ActiveBehavior[] {
    const t = new Date(ts).getTime();
    return behaviors.filter((ab) => {
        const start = new Date(ab.start_time).getTime();
        const end = new Date(ab.end_time).getTime();
        return start <= t && t <= end;
    });
}
export function MetadataDetailPage() {
    const { vin, startTime } = useParams({
        from: "/metadata/$vin/$startTime",
    });

    const { data, isLoading, error } = useQuery({
        queryKey: ["metadata", vin, startTime],
        queryFn: () => metadataApi.get(vin, startTime),
    });

    const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
    const mapRef = useRef<MapRef>(null);
    const mapContainerRef = useRef<HTMLDivElement>(null);
    const mapLoadedRef = useRef(false);

    // Build unique sorted timestamps from camera frames
    const timestamps = useMemo(() => {
        if (!data) return [];
        const tsSet = new Set((data.camera_frames ?? []).map((f: CameraFrameRef) => f.ts));
        return Array.from(tsSet).sort();
    }, [data]);

    // Build a lookup: camera_id -> ts -> CameraFrameRef
    const frameMap = useMemo(() => {
        if (!data) return new Map<string, Map<string, CameraFrameRef>>();
        const map = new Map<string, Map<string, CameraFrameRef>>();
        for (const frame of data.camera_frames ?? []) {
            if (!map.has(frame.camera_id)) {
                map.set(frame.camera_id, new Map());
            }
            map.get(frame.camera_id)!.set(frame.ts, frame);
        }
        return map;
    }, [data]);

    // Find the trajectory index closest to a given timestamp
    const findClosestTrajectoryIndex = (ts: string): number | null => {
        if (!data || data.trajectory.length === 0) return null;
        const target = new Date(ts).getTime();
        let closest = 0;
        let minDiff = Infinity;
        for (let i = 0; i < data.trajectory.length; i++) {
            const diff = Math.abs(new Date(data.trajectory[i].timestamp).getTime() - target);
            if (diff < minDiff) {
                minDiff = diff;
                closest = i;
            }
        }
        return closest;
    };

    // Find the closest timestamp to a trajectory point
    const findClosestTimestamp = (index: number): string | null => {
        if (!data || timestamps.length === 0) return null;
        const pointTime = new Date(data.trajectory[index].timestamp).getTime();
        let closest = timestamps[0];
        let minDiff = Infinity;
        for (const ts of timestamps) {
            const diff = Math.abs(new Date(ts).getTime() - pointTime);
            if (diff < minDiff) {
                minDiff = diff;
                closest = ts;
            }
        }
        return closest;
    };

    // Highlighted timestamp derived from selected trajectory index
    const highlightedTs = selectedIndex != null ? findClosestTimestamp(selectedIndex) : null;

    const handleTimestampClick = (ts: string) => {
        const idx = findClosestTrajectoryIndex(ts);
        setSelectedIndex(idx);
    };

    const handleMapPointClick = (index: number) => {
        setSelectedIndex(index);
    };

    // GeoJSON data for the trajectory line
    const trajectoryGeoJSON = useMemo(
        () => ({
            type: "FeatureCollection" as const,
            features: data
                ? [
                    {
                        type: "Feature" as const,
                        geometry: {
                            type: "LineString" as const,
                            coordinates: (data.trajectory as LocationFix[]).map(
                                (f) => [f.longitude, f.latitude],
                            ),
                        },
                        properties: {},
                    },
                ]
                : [],
        }),
        [data],
    );

    // GeoJSON data for individual trajectory points with index property for styling
    const pointsGeoJSON = useMemo(
        () => ({
            type: "FeatureCollection" as const,
            features: data
                ? (data.trajectory as LocationFix[]).map((f, i) => ({
                    type: "Feature" as const,
                    geometry: {
                        type: "Point" as const,
                        coordinates: [f.longitude, f.latitude],
                    },
                    properties: { index: i },
                }))
                : [],
        }),
        [data],
    );

    // Click on the map: query which trajectory point was clicked
    const handleMapClick = useCallback(
        (e: MapMouseEvent) => {
            const features = mapRef.current?.queryRenderedFeatures(e.point, {
                layers: ["trajectory-points"],
            });
            if (features && features.length > 0) {
                const idx = features[0].properties?.index as number | undefined;
                if (idx !== undefined) handleMapPointClick(idx);
            }
        },
        // eslint-disable-next-line react-hooks/exhaustive-deps
        [],
    );

    // Fit map to trajectory bounds
    const fitMapToBounds = useCallback(() => {
        if (!mapRef.current || !data || data.trajectory.length === 0) return;
        mapRef.current.resize();
        const traj = data.trajectory as LocationFix[];
        const bounds = traj.slice(1).reduce(
            (b, f) => b.extend([f.longitude, f.latitude] as [number, number]),
            new LngLatBounds(
                [traj[0].longitude, traj[0].latitude],
                [traj[0].longitude, traj[0].latitude],
            ),
        );
        mapRef.current.fitBounds(bounds, { padding: 40, duration: 0 });
    }, [data]);

    // Fit once the map style has loaded
    const handleMapLoad = useCallback(() => {
        mapLoadedRef.current = true;
        requestAnimationFrame(fitMapToBounds);
        window.setTimeout(fitMapToBounds, 100);
        window.setTimeout(fitMapToBounds, 500);
    }, [fitMapToBounds]);

    // If data arrives after the map is already mounted (cached map + slow API), fit then too
    useEffect(() => {
        if (mapLoadedRef.current) fitMapToBounds();
    }, [fitMapToBounds]);

    useEffect(() => {
        const container = mapContainerRef.current;
        if (!container) return;

        const refresh = () => {
            mapRef.current?.resize();
            if (mapLoadedRef.current) fitMapToBounds();
        };

        const frame = requestAnimationFrame(refresh);
        const timeout = window.setTimeout(refresh, 250);
        const observer = typeof ResizeObserver === "undefined"
            ? null
            : new ResizeObserver(refresh);
        observer?.observe(container);

        return () => {
            cancelAnimationFrame(frame);
            window.clearTimeout(timeout);
            observer?.disconnect();
        };
    }, [fitMapToBounds]);

    if (isLoading) {
        return <p className="text-sm text-gray-500">Loading...</p>;
    }

    if (error || !data) {
        return (
            <div className="space-y-4">
                <Link
                    to="/metadata"
                    className="text-sm text-indigo-600 hover:underline"
                >
                    ← Back to Metadata
                </Link>
                <p className="text-sm text-red-600">
                    Failed to load metadata: {String(error ?? "Not found")}
                </p>
            </div>
        );
    }

    const hasTrajectory = data.trajectory.length > 0;

    return (
        <div className="space-y-4">
            <div className="flex items-center gap-4">
                <Link
                    to="/metadata"
                    className="text-sm text-indigo-600 hover:underline"
                >
                    ← Back
                </Link>
                <h1 className="text-xl font-bold text-gray-900">{data.vin}</h1>
                <span className="text-sm text-gray-500">
                    {formatDateTime(data.start_time)} — {formatDateTime(data.end_time)}
                </span>
                <span className="text-xs text-gray-400">Model: {data.model}</span>
                {(data.active_behaviors ?? []).length > 0 && (
                    <div className="flex flex-wrap gap-1">
                        {(data.active_behaviors ?? []).map((ab, i) => (
                            <span
                                key={i}
                                title={`${formatDateTime(ab.start_time)} — ${formatDateTime(ab.end_time)}`}
                                className="inline-flex items-center rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800"
                            >
                                {ab.name}
                            </span>
                        ))}
                    </div>
                )}
            </div>

            <div className="flex gap-4" style={{ height: "calc(100vh - 160px)" }}>
                {/* Left: Map */}
                <div ref={mapContainerRef} className="w-1/2 overflow-hidden rounded-lg border border-gray-200">
                    {hasTrajectory ? (
                        <MapGL
                            key={`${vin}-${startTime}`}
                            ref={mapRef}
                            initialViewState={{
                                longitude: data.trajectory[0].longitude,
                                latitude: data.trajectory[0].latitude,
                                zoom: 13,
                            }}
                            mapStyle={MAP_STYLE}
                            style={{ height: "100%", width: "100%" }}
                            interactiveLayerIds={["trajectory-points"]}
                            onClick={handleMapClick}
                            onLoad={handleMapLoad}
                        >
                            <Source id="trajectory" type="geojson" data={trajectoryGeoJSON}>
                                <Layer
                                    id="trajectory-line"
                                    type="line"
                                    paint={{ "line-color": "#4f46e5", "line-width": 3 }}
                                />
                            </Source>
                            <Source id="points" type="geojson" data={pointsGeoJSON}>
                                <Layer
                                    id="trajectory-points"
                                    type="circle"
                                    paint={{
                                        "circle-radius": [
                                            "case",
                                            ["==", ["get", "index"], selectedIndex ?? -1],
                                            10,
                                            6,
                                        ],
                                        "circle-color": [
                                            "case",
                                            ["==", ["get", "index"], selectedIndex ?? -1],
                                            "#dc2626",
                                            "#818cf8",
                                        ],
                                        "circle-stroke-color": [
                                            "case",
                                            ["==", ["get", "index"], selectedIndex ?? -1],
                                            "#dc2626",
                                            "#4f46e5",
                                        ],
                                        "circle-stroke-width": [
                                            "case",
                                            ["==", ["get", "index"], selectedIndex ?? -1],
                                            3,
                                            1,
                                        ],
                                        "circle-opacity": [
                                            "case",
                                            ["==", ["get", "index"], selectedIndex ?? -1],
                                            1,
                                            0.8,
                                        ],
                                    }}
                                />
                            </Source>
                        </MapGL>
                    ) : (
                        <div className="flex h-full items-center justify-center bg-gray-100 text-sm text-gray-400">
                            No trajectory data
                        </div>
                    )}
                </div>

                {/* Right: Image matrix */}
                <div className="w-1/2 overflow-auto rounded-lg border border-gray-200 bg-white">
                    {timestamps.length === 0 ? (
                        <div className="flex h-full items-center justify-center text-sm text-gray-400">
                            No camera images
                        </div>
                    ) : (
                        <table className="w-full text-xs">
                            <thead className="sticky top-0 z-10 bg-gray-50">
                                <tr>
                                    <th className="px-2 py-1.5 text-left font-medium text-gray-600">
                                        Time
                                    </th>
                                    {CAMERA_ORDER.map((cam) => (
                                        <th
                                            key={cam}
                                            className="px-1 py-1.5 text-center font-medium text-gray-600"
                                        >
                                            {cam.replace("_", " ")}
                                        </th>
                                    ))}
                                </tr>
                            </thead>
                            <tbody>
                                {timestamps.map((ts) => {
                                    const isHighlighted = ts === highlightedTs;
                                    return (
                                        <tr
                                            key={ts}
                                            onClick={() => handleTimestampClick(ts)}
                                            className={`cursor-pointer border-t border-gray-100 transition-colors ${isHighlighted
                                                ? "bg-red-50 ring-1 ring-inset ring-red-300"
                                                : "hover:bg-gray-50"
                                                }`}
                                        >
                                            <td className="px-2 py-1">
                                                <div className="whitespace-nowrap font-mono text-gray-700">
                                                    {formatTime(ts)}
                                                </div>
                                                {(() => {
                                                    const abs = getActiveBehaviorsAt(ts, data.active_behaviors ?? []);
                                                    return abs.length > 0 ? (
                                                        <div className="mt-0.5 flex flex-wrap gap-0.5">
                                                            {abs.map((ab, i) => (
                                                                <span
                                                                    key={i}
                                                                    title={`${formatDateTime(ab.start_time)} — ${formatDateTime(ab.end_time)}`}
                                                                    className="inline-flex items-center rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800"
                                                                >
                                                                    {ab.name}
                                                                </span>
                                                            ))}
                                                        </div>
                                                    ) : null;
                                                })()}
                                            </td>
                                            {CAMERA_ORDER.map((cam) => {
                                                const frame = frameMap
                                                    .get(cam)
                                                    ?.get(ts);
                                                return (
                                                    <td
                                                        key={cam}
                                                        className="px-1 py-1"
                                                    >
                                                        {frame ? (
                                                            <img
                                                                src={frame.image_url}
                                                                alt={`${cam} ${ts}`}
                                                                className={`h-16 w-24 rounded object-cover ${isHighlighted
                                                                    ? "ring-2 ring-red-500"
                                                                    : ""
                                                                    }`}
                                                                loading="lazy"
                                                            />
                                                        ) : (
                                                            <div className="flex h-16 w-24 items-center justify-center rounded bg-gray-100 text-gray-300">
                                                                —
                                                            </div>
                                                        )}
                                                    </td>
                                                );
                                            })}
                                        </tr>
                                    );
                                })}
                            </tbody>
                        </table>
                    )}
                </div>
            </div>
        </div>
    );
}
