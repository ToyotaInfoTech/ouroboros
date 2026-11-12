// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useInfiniteQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { metadata } from "../../api/client";
import type { MetadataListItem, MetadataListParams } from "../../api/types";
import Map, { Source, Layer, Marker } from "react-map-gl/maplibre";
import type { MapRef, MapMouseEvent } from "react-map-gl/maplibre";
import { useState, useRef, useEffect, useCallback, useMemo } from "react";
import { Check, MapPin, X } from "lucide-react";
import { useMetadataFilterStore } from "../../stores/metadataFilterStore";
import type { GeoBounds } from "../../stores/metadataFilterStore";
import {
    MAP_BACKGROUND_COLOR,
    MAP_STYLE,
    MAP_TILE_ATTRIBUTION,
    MAP_TILE_URL,
    attributionToText,
    boundsToGeoJSON,
    tileImageUrl,
} from "../../lib/mapUtils";

const PAGE_SIZE = 20;

function formatDateTime(iso: string): string {
    try {
        return new Date(iso).toLocaleString();
    } catch {
        return iso;
    }
}

type ThumbnailBounds = {
    minLng: number;
    minLat: number;
    maxLng: number;
    maxLat: number;
};

function expandedThumbnailBounds(item: MetadataListItem): ThumbnailBounds | null {
    if (!item.bbox) return null;

    let minLng = item.bbox.min_lng;
    let minLat = item.bbox.min_lat;
    let maxLng = item.bbox.max_lng;
    let maxLat = item.bbox.max_lat;

    for (const fix of item.trajectory) {
        minLng = Math.min(minLng, fix.longitude);
        minLat = Math.min(minLat, fix.latitude);
        maxLng = Math.max(maxLng, fix.longitude);
        maxLat = Math.max(maxLat, fix.latitude);
    }

    const lngSpan = Math.max(maxLng - minLng, 0.0001);
    const latSpan = Math.max(maxLat - minLat, 0.0001);
    const lngPad = lngSpan * 0.12;
    const latPad = latSpan * 0.12;

    return {
        minLng: minLng - lngPad,
        minLat: minLat - latPad,
        maxLng: maxLng + lngPad,
        maxLat: maxLat + latPad,
    };
}


// ── Canvas-based map thumbnail (avoids WebGL context exhaustion) ───────────

/** Module-level tile cache: keyed "z/x/y" → loaded image, "loading", or "error".
 * Uses globalThis.Map to avoid conflict with the react-map-gl Map component import. */
const tileCache = new globalThis.Map<string, HTMLImageElement | "loading" | "error">();

/**
 * Pending callbacks for in-flight tile requests.
 * Multiple canvases can register for the same tile; when it loads all are notified.
 * Keyed "z/x/y" → list of redraw callbacks.
 */
const tilePending = new globalThis.Map<string, Array<() => void>>();

/** Longitude → world pixel X at zoom z (Mercator). */
function lngToWorldPx(lng: number, zoom: number): number {
    return ((lng + 180) / 360) * 256 * (1 << zoom);
}

/** Latitude → world pixel Y at zoom z (Mercator). */
function latToWorldPy(lat: number, zoom: number): number {
    const r = Math.PI / 180;
    return (
        (1 - Math.log(Math.tan(lat * r) + 1 / Math.cos(lat * r)) / Math.PI) /
        2 *
        256 *
        (1 << zoom)
    );
}

/** Choose zoom so the viewport fits inside the canvas without distortion.
 *
 * Square Mercator tiles are assumed.  We must honour both the horizontal
 * (longitude) and vertical (latitude / Mercator-Y) extents of the bounding
 * box against the canvas dimensions simultaneously; otherwise tiles are
 * scaled non-uniformly and appear stretched.
 *
 * Strategy: compute the largest zoom level such that:
 *   - the lng-span fits within canvasW, AND
 *   - the Mercator-Y-span fits within canvasH
 * Then take the minimum (most zoomed-out) of the two candidates so that the
 * entire bbox is visible and pixel aspect ratio is preserved.
 */
function pickZoom(bounds: ThumbnailBounds, canvasW: number, canvasH: number): number {
    const lngSpan = bounds.maxLng - bounds.minLng;

    const r = Math.PI / 180;
    const mercatorY = (lat: number) =>
        (1 - Math.log(Math.tan(lat * r) + 1 / Math.cos(lat * r)) / Math.PI) / 2;
    const Δmercator_fraction = Math.abs(mercatorY(bounds.minLat) - mercatorY(bounds.maxLat));

    // 横方向: lngSpan が canvasW に収まるズーム
    const zLng = Math.log2((canvasW * 360) / (256 * lngSpan));

    // 縦方向: Mercator-Y スパンが canvasH に収まるズーム
    const zLat = Math.log2(canvasH / (256 * Δmercator_fraction));

    // 両方収まる方を選ぶ（min でアスペクト比を維持）
    return Math.max(2, Math.min(17, Math.floor(Math.min(zLng, zLat))));
}

/**
 * Draw raster tiles (when a tile URL is configured) + bbox overlay + trajectory
 * onto a canvas element.
 * Uses only Canvas 2D — no WebGL context is created, so this is safe to call
 * for any number of simultaneous thumbnails.
 * When a tile image finishes loading, `onTileLoaded` is called so the caller
 * can redraw with the newly cached tile.
 */
function drawMapThumbnail(
    canvas: HTMLCanvasElement,
    bounds: ThumbnailBounds,
    trajectory: Array<{ longitude: number; latitude: number }>,
    bbox: { min_lng: number; min_lat: number; max_lng: number; max_lat: number } | null,
    onTileLoaded: () => void,
): void {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Use physical canvas pixels throughout — canvas.width/height are already
    // set to the physical-pixel dimensions by MiniMapThumbnail's useEffect.
    // No DPR division or ctx.scale() needed; everything is in physical pixels.
    const W = canvas.width;
    const H = canvas.height;
    const zoom = pickZoom(bounds, W, H);

    // Viewport in world pixels (bounding box extent at this zoom)
    const vpLeft0 = lngToWorldPx(bounds.minLng, zoom);
    const vpRight0 = lngToWorldPx(bounds.maxLng, zoom);
    const vpTop0 = latToWorldPy(bounds.maxLat, zoom);
    const vpBottom0 = latToWorldPy(bounds.minLat, zoom);
    const vpW0 = vpRight0 - vpLeft0;
    const vpH0 = vpBottom0 - vpTop0;

    // Expand the viewport to match the canvas aspect ratio so tiles fill the
    // canvas completely — no letterboxing, uniform scale.
    // Tiles are square (256×256), so the only way to keep tiles square
    // while filling the canvas is to use a single scale factor for both axes
    // and expand the shorter side to match.
    const vpCx = (vpLeft0 + vpRight0) / 2;
    const vpCy = (vpTop0 + vpBottom0) / 2;
    const canvasAspect = W / H;
    const vpAspect = vpW0 / vpH0;
    const vpW = vpAspect >= canvasAspect ? vpW0 : vpH0 * canvasAspect;
    const vpH = vpAspect >= canvasAspect ? vpW0 / canvasAspect : vpH0;
    const vpLeft = vpCx - vpW / 2;
    const vpRight = vpCx + vpW / 2;
    const vpTop = vpCy - vpH / 2;
    const vpBottom = vpCy + vpH / 2;

    // Uniform scale: W / vpW == H / vpH (exact after the expansion above).
    const scale = W / vpW;

    // World-pixel → canvas-pixel (uniform scale, centred on the bbox)
    const cx = (wpx: number) => (wpx - vpLeft) * scale;
    const cy = (wpy: number) => (wpy - vpTop) * scale;

    // Flat background; tiles (if configured) are drawn on top of it.
    ctx.fillStyle = MAP_BACKGROUND_COLOR;
    ctx.fillRect(0, 0, W, H);

    // Draw tiles — only when a tile URL is configured, so that no third-party
    // tile server is contacted by default.
    const tSize = 256;
    const maxTile = (1 << zoom) - 1;
    const tLeft = Math.floor(vpLeft / tSize);
    const tRight = Math.floor(vpRight / tSize);
    const tTop = Math.floor(vpTop / tSize);
    const tBottom = Math.floor(vpBottom / tSize);
    // All tiles are square with the same pixel size under the uniform scale.
    const tilePixels = tSize * scale;

    if (MAP_TILE_URL) {
        for (let tx = tLeft; tx <= tRight; tx++) {
            for (let ty = tTop; ty <= tBottom; ty++) {
                if (ty < 0 || ty > maxTile) continue;
                const wrappedTx = ((tx % (1 << zoom)) + (1 << zoom)) % (1 << zoom);
                const key = `${zoom}/${wrappedTx}/${ty}`;

                const tileX = cx(tx * tSize);
                const tileY = cy(ty * tSize);
                const tileW = tilePixels;
                const tileH = tilePixels;

                const cached = tileCache.get(key);
                if (cached instanceof HTMLImageElement) {
                    ctx.drawImage(cached, tileX, tileY, tileW, tileH);
                } else if (cached === "loading") {
                    // Another card already requested this tile — just register our callback.
                    const pending = tilePending.get(key);
                    if (pending && !pending.includes(onTileLoaded)) {
                        pending.push(onTileLoaded);
                    }
                } else if (!cached) {
                    // First requester: start loading and register callback list.
                    tileCache.set(key, "loading");
                    tilePending.set(key, [onTileLoaded]);
                    const img = new Image();
                    img.crossOrigin = "anonymous";
                    img.src = tileImageUrl(MAP_TILE_URL, zoom, wrappedTx, ty);
                    img.onload = () => {
                        tileCache.set(key, img);
                        const cbs = tilePending.get(key) ?? [];
                        tilePending.delete(key);
                        cbs.forEach(cb => cb());
                    };
                    img.onerror = () => {
                        tileCache.set(key, "error");
                        const cbs = tilePending.get(key) ?? [];
                        tilePending.delete(key);
                        cbs.forEach(cb => cb());
                    };
                }
            }
        }
    }

    // Bbox fill + outline
    if (bbox) {
        const bx1 = cx(lngToWorldPx(bbox.min_lng, zoom));
        const bx2 = cx(lngToWorldPx(bbox.max_lng, zoom));
        const by1 = cy(latToWorldPy(bbox.max_lat, zoom));
        const by2 = cy(latToWorldPy(bbox.min_lat, zoom));
        ctx.fillStyle = "rgba(79, 70, 229, 0.15)";
        ctx.fillRect(bx1, by1, bx2 - bx1, by2 - by1);
        ctx.strokeStyle = "#4f46e5";
        ctx.lineWidth = 1.5;
        ctx.strokeRect(bx1, by1, bx2 - bx1, by2 - by1);
    }

    // Trajectory line
    if (trajectory.length >= 2) {
        ctx.beginPath();
        for (let i = 0; i < trajectory.length; i++) {
            const x = cx(lngToWorldPx(trajectory[i].longitude, zoom));
            const y = cy(latToWorldPy(trajectory[i].latitude, zoom));
            if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        }
        ctx.strokeStyle = "#4f46e5";
        ctx.lineWidth = 2;
        ctx.lineJoin = "round";
        ctx.lineCap = "round";
        ctx.stroke();
    }
}

function MiniMapThumbnail({ item }: { item: MetadataListItem }) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const bounds = useMemo(() => expandedThumbnailBounds(item), [item]);
    const cancelledRef = useRef(false);

    useEffect(() => {
        cancelledRef.current = false;
        if (!bounds || !canvasRef.current) return;
        const canvas = canvasRef.current;

        // Set canvas buffer to physical pixels matching the h-32 w-48 container.
        // Drawing in drawMapThumbnail uses canvas.width/height directly (physical
        // pixels) — no ctx.scale() is applied, so no DPR coordinate confusion.
        const dpr = window.devicePixelRatio || 1;
        canvas.width = Math.round(192 * dpr);
        canvas.height = Math.round(128 * dpr);

        const redraw = () => {
            if (!cancelledRef.current) {
                drawMapThumbnail(canvas, bounds, item.trajectory, item.bbox ?? null, redraw);
            }
        };
        redraw();
        return () => { cancelledRef.current = true; };
    }, [bounds, item]);

    if (!bounds) {
        return (
            <div className="flex h-full items-center justify-center bg-gray-100 text-xs text-gray-400">
                No location
            </div>
        );
    }

    return (
        <canvas
            ref={canvasRef}
            className="h-full w-full"
        />
    );
}

// ── Datetime picker with OK / Cancel ───────────────────────────────────

function DateTimePicker({
    label,
    value,
    onChange,
}: {
    label: string;
    value: string;
    onChange: (v: string) => void;
}) {
    const [open, setOpen] = useState(false);
    const [draft, setDraft] = useState(value);
    const wrapRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!open) return;
        const handler = (e: MouseEvent) => {
            if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
                setOpen(false);
            }
        };
        document.addEventListener("pointerdown", handler);
        return () => document.removeEventListener("pointerdown", handler);
    }, [open]);

    return (
        <div ref={wrapRef} className="relative flex flex-col gap-1 text-xs font-medium text-gray-600">
            {label}
            <button
                type="button"
                onClick={() => { setDraft(value); setOpen(!open); }}
                className="flex items-center gap-1 rounded border border-gray-300 px-2 py-1 text-sm text-left min-w-[10rem] hover:bg-gray-50"
            >
                <span className={value ? "text-gray-900" : "text-gray-400"}>
                    {value ? new Date(value).toLocaleString() : "—"}
                </span>
                {value && (
                    <button
                        type="button"
                        onClick={(e) => { e.stopPropagation(); onChange(""); setOpen(false); }}
                        className="ml-auto text-gray-400 hover:text-gray-600"
                        aria-label="Clear date filter"
                    >
                        <X className="h-3 w-3" />
                    </button>
                )}
            </button>
            {open && (
                <div className="absolute top-full left-0 z-[10001] mt-1 flex flex-col gap-1.5 rounded-lg border border-gray-200 bg-white p-2 shadow-lg">
                    <input
                        type="datetime-local"
                        autoFocus
                        value={draft}
                        onChange={(e) => setDraft(e.target.value)}
                        className="rounded border border-gray-300 px-2 py-1 text-sm"
                    />
                    <div className="flex justify-end gap-1">
                        <button
                            type="button"
                            onClick={() => setOpen(false)}
                            className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
                            title="Cancel"
                        >
                            <X className="h-3.5 w-3.5" />
                        </button>
                        <button
                            type="button"
                            onClick={() => { onChange(draft); setOpen(false); }}
                            className="rounded p-1 text-indigo-600 hover:bg-indigo-50"
                            title="OK"
                        >
                            <Check className="h-3.5 w-3.5" />
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}

// ── Geo bounds picker (drag to draw rectangle, move & resize) ──────────

type DrawInteraction = "idle" | "drawing" | "moving" | "resizing";
type HandlePos = "nw" | "ne" | "sw" | "se";

/** Bounds as [minLng, minLat, maxLng, maxLat] — maplibre-gl order. */
type LngLatBoundsArray = [number, number, number, number];

function RectanglePicker({
    onConfirm,
    onCancel,
    initial,
}: {
    onConfirm: (b: GeoBounds) => void;
    onCancel: () => void;
    initial?: GeoBounds | null;
}) {
    const [bounds, setBounds] = useState<LngLatBoundsArray | null>(() => {
        if (initial) {
            return [initial.min_lng, initial.min_lat, initial.max_lng, initial.max_lat];
        }
        return null;
    });

    const [drawMode, setDrawMode] = useState(true);
    const interactionRef = useRef<DrawInteraction>("idle");
    const [interaction, setInteraction] = useState<DrawInteraction>("idle");

    const mapRef = useRef<MapRef>(null);
    const containerRef = useRef<HTMLDivElement>(null);

    // Refs for ongoing interaction state
    const drawOriginRef = useRef<{ lng: number; lat: number } | null>(null);
    const moveStateRef = useRef<{
        start: { lng: number; lat: number };
        bounds: LngLatBoundsArray;
    } | null>(null);
    const resizeAnchorRef = useRef<{ lng: number; lat: number } | null>(null);

    /** Convert client pixel coordinates to map lng/lat. */
    const toMapCoords = useCallback(
        (clientX: number, clientY: number): { lng: number; lat: number } | null => {
            if (!containerRef.current || !mapRef.current) return null;
            const rect = containerRef.current.getBoundingClientRect();
            const pt = mapRef.current.unproject([clientX - rect.left, clientY - rect.top]);
            return { lng: pt.lng, lat: pt.lat };
        },
        [],
    );

    // Document-level pointer move/up handles all ongoing interactions
    useEffect(() => {
        const onMove = (e: PointerEvent) => {
            const mode = interactionRef.current;
            if (mode === "idle") return;
            const pt = toMapCoords(e.clientX, e.clientY);
            if (!pt) return;

            if (mode === "drawing" && drawOriginRef.current) {
                const o = drawOriginRef.current;
                setBounds([
                    Math.min(o.lng, pt.lng),
                    Math.min(o.lat, pt.lat),
                    Math.max(o.lng, pt.lng),
                    Math.max(o.lat, pt.lat),
                ]);
            } else if (mode === "moving" && moveStateRef.current) {
                const { start, bounds: b } = moveStateRef.current;
                const dLng = pt.lng - start.lng;
                const dLat = pt.lat - start.lat;
                setBounds([b[0] + dLng, b[1] + dLat, b[2] + dLng, b[3] + dLat]);
            } else if (mode === "resizing" && resizeAnchorRef.current) {
                const a = resizeAnchorRef.current;
                setBounds([
                    Math.min(a.lng, pt.lng),
                    Math.min(a.lat, pt.lat),
                    Math.max(a.lng, pt.lng),
                    Math.max(a.lat, pt.lat),
                ]);
            }
        };

        const onUp = () => {
            interactionRef.current = "idle";
            setInteraction("idle");
            drawOriginRef.current = null;
            moveStateRef.current = null;
            resizeAnchorRef.current = null;
        };

        document.addEventListener("pointermove", onMove);
        document.addEventListener("pointerup", onUp);
        return () => {
            document.removeEventListener("pointermove", onMove);
            document.removeEventListener("pointerup", onUp);
        };
    }, [toMapCoords]);

    // Map mousedown: start drawing or moving
    const handleMapMouseDown = useCallback(
        (e: MapMouseEvent) => {
            if (!drawMode || interactionRef.current !== "idle") return;
            e.preventDefault();
            const lngLat = { lng: e.lngLat.lng, lat: e.lngLat.lat };

            if (
                bounds &&
                lngLat.lng >= bounds[0] &&
                lngLat.lng <= bounds[2] &&
                lngLat.lat >= bounds[1] &&
                lngLat.lat <= bounds[3]
            ) {
                // Click inside existing rectangle → move
                interactionRef.current = "moving";
                setInteraction("moving");
                moveStateRef.current = { start: lngLat, bounds };
            } else {
                // Click outside → draw new rectangle
                interactionRef.current = "drawing";
                setInteraction("drawing");
                drawOriginRef.current = lngLat;
                setBounds(null);
            }
        },
        [drawMode, bounds],
    );

    // Corner handle pointerdown: start resizing
    const handleResizeDown = useCallback(
        (pos: HandlePos, e: React.PointerEvent) => {
            e.stopPropagation();
            if (!bounds || interactionRef.current !== "idle") return;
            const anchorLng = pos.includes("e") ? bounds[0] : bounds[2];
            const anchorLat = pos.includes("n") ? bounds[1] : bounds[3];
            resizeAnchorRef.current = { lng: anchorLng, lat: anchorLat };
            interactionRef.current = "resizing";
            setInteraction("resizing");
        },
        [bounds],
    );

    // Esc key: clear bounds or cancel
    useEffect(() => {
        const handler = (e: KeyboardEvent) => {
            if (e.key === "Escape") {
                e.preventDefault();
                if (bounds) setBounds(null);
                else onCancel();
            }
        };
        document.addEventListener("keydown", handler);
        return () => document.removeEventListener("keydown", handler);
    }, [bounds, onCancel]);

    const rectGeoJSON = useMemo(
        () => (bounds ? boundsToGeoJSON(...bounds) : null),
        [bounds],
    );

    const statusText = !bounds
        ? drawMode
            ? "Drag to draw a rectangle"
            : "Click Draw to enter draw mode"
        : interaction === "drawing"
            ? "Release to finish drawing"
            : interaction === "moving"
                ? "Release to finish moving"
                : interaction === "resizing"
                    ? "Release to finish resizing"
                    : drawMode
                        ? "Drag rectangle to move · corners to resize · Esc to clear"
                        : "Click Draw to modify the selection";

    const mapCursor =
        interaction === "moving" || interaction === "resizing"
            ? "grabbing"
            : drawMode
                ? "crosshair"
                : "grab";

    const initialViewState = initial
        ? {
            bounds: [
                initial.min_lng,
                initial.min_lat,
                initial.max_lng,
                initial.max_lat,
            ] as [number, number, number, number],
            fitBoundsOptions: { padding: 20 },
        }
        : { longitude: 136.9, latitude: 35.18, zoom: 11 };

    return (
        <div className="fixed inset-0 z-[10000] flex items-center justify-center bg-black/40">
            <div className="flex w-[700px] flex-col rounded-lg bg-white shadow-xl">
                <div className="flex items-center justify-between border-b px-4 py-2">
                    <span className="text-sm font-semibold text-gray-800">
                        {statusText}
                    </span>
                    <button
                        onClick={onCancel}
                        className="text-gray-400 hover:text-gray-600"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </div>
                <div
                    ref={containerRef}
                    className="relative h-[450px]"
                    style={{ cursor: mapCursor }}
                >
                    <Map
                        ref={mapRef}
                        initialViewState={initialViewState}
                        mapStyle={MAP_STYLE}
                        style={{ height: "100%", width: "100%", cursor: "inherit" }}
                        dragPan={!drawMode}
                        onMouseDown={handleMapMouseDown}
                        attributionControl={MAP_TILE_ATTRIBUTION ? undefined : false}
                    >
                        {rectGeoJSON && (
                            <>
                                <Source id="rect" type="geojson" data={rectGeoJSON}>
                                    <Layer
                                        id="rect-fill"
                                        type="fill"
                                        paint={{
                                            "fill-color": "#4f46e5",
                                            "fill-opacity": 0.15,
                                        }}
                                    />
                                    <Layer
                                        id="rect-outline"
                                        type="line"
                                        paint={{
                                            "line-color": "#4f46e5",
                                            "line-width": 2,
                                        }}
                                    />
                                </Source>
                                {drawMode &&
                                    bounds &&
                                    (["nw", "ne", "sw", "se"] as HandlePos[]).map((pos) => {
                                        const lng = pos.includes("e") ? bounds[2] : bounds[0];
                                        const lat = pos.includes("n") ? bounds[3] : bounds[1];
                                        const cursor =
                                            pos === "nw" || pos === "se"
                                                ? "nwse-resize"
                                                : "nesw-resize";
                                        return (
                                            <Marker
                                                key={pos}
                                                longitude={lng}
                                                latitude={lat}
                                                anchor="center"
                                            >
                                                <div
                                                    style={{
                                                        width: 12,
                                                        height: 12,
                                                        borderRadius: "50%",
                                                        background: "#4f46e5",
                                                        border: "2px solid white",
                                                        cursor,
                                                        touchAction: "none",
                                                    }}
                                                    onPointerDown={(e) =>
                                                        handleResizeDown(pos, e)
                                                    }
                                                />
                                            </Marker>
                                        );
                                    })}
                            </>
                        )}
                    </Map>
                </div>
                <div className="flex items-center gap-2 border-t px-4 py-2">
                    <button
                        type="button"
                        onClick={() => setDrawMode((m) => !m)}
                        className={`rounded px-3 py-1.5 text-xs font-medium ${drawMode
                            ? "bg-indigo-600 text-white hover:bg-indigo-700"
                            : "border border-gray-300 text-gray-600 hover:bg-gray-50"
                            }`}
                    >
                        {drawMode ? "Drawing" : "Draw"}
                    </button>
                    <span className="text-xs text-gray-400">Scroll to zoom</span>
                    <div className="ml-auto flex items-center gap-2">
                        <button
                            onClick={onCancel}
                            className="rounded px-3 py-1.5 text-xs text-gray-600 hover:bg-gray-100"
                        >
                            Cancel
                        </button>
                        <button
                            disabled={!bounds}
                            onClick={() => {
                                if (bounds) {
                                    onConfirm({
                                        min_lng: bounds[0],
                                        min_lat: bounds[1],
                                        max_lng: bounds[2],
                                        max_lat: bounds[3],
                                    });
                                }
                            }}
                            className="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-700 disabled:opacity-40"
                        >
                            Apply
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
}

export function MetadataPage() {
    const navigate = useNavigate();

    // Filter state from zustand store (persists across navigation)
    const vinPattern = useMetadataFilterStore((s) => s.vinPattern);
    const setVinPattern = useMetadataFilterStore((s) => s.setVinPattern);
    const modelFilter = useMetadataFilterStore((s) => s.modelFilter);
    const setModelFilter = useMetadataFilterStore((s) => s.setModelFilter);
    const timeStart = useMetadataFilterStore((s) => s.timeStart);
    const setTimeStart = useMetadataFilterStore((s) => s.setTimeStart);
    const timeEnd = useMetadataFilterStore((s) => s.timeEnd);
    const setTimeEnd = useMetadataFilterStore((s) => s.setTimeEnd);
    const geoBounds = useMetadataFilterStore((s) => s.geoBounds);
    const setGeoBounds = useMetadataFilterStore((s) => s.setGeoBounds);
    const [showMapPicker, setShowMapPicker] = useState(false);

    // Build filter params for query key + fetch
    const filterParams: Omit<MetadataListParams, "limit" | "offset"> = {
        ...(vinPattern ? { vin_pattern: vinPattern } : {}),
        ...(modelFilter ? { model: modelFilter } : {}),
        ...(timeStart ? { time_start: new Date(timeStart).toISOString() } : {}),
        ...(timeEnd ? { time_end: new Date(timeEnd).toISOString() } : {}),
        ...(geoBounds ?? {}),
    };

    const {
        data,
        isLoading,
        error,
        fetchNextPage,
        hasNextPage,
        isFetchingNextPage,
    } = useInfiniteQuery({
        queryKey: ["metadata", filterParams],
        queryFn: ({ pageParam = 0 }) =>
            metadata.list({ ...filterParams, limit: PAGE_SIZE, offset: pageParam }),
        initialPageParam: 0,
        getNextPageParam: (lastPage, _allPages, lastPageParam) =>
            lastPage.length < PAGE_SIZE ? undefined : lastPageParam + PAGE_SIZE,
        // Auto-refresh every 5 s while fewer than one full page of results
        // so newly ingested records appear without manual reload.
        refetchInterval: (query) => {
            const pages = query.state.data?.pages;
            const totalItems = pages?.reduce((n, p) => n + p.length, 0) ?? 0;
            return totalItems < PAGE_SIZE ? 5000 : false;
        },
    });

    const items: MetadataListItem[] = data?.pages.flatMap((p) => p) ?? [];

    // Infinite scroll sentinel
    const sentinelRef = useRef<HTMLDivElement>(null);
    const handleIntersect = useCallback(
        (entries: IntersectionObserverEntry[]) => {
            if (entries[0]?.isIntersecting && hasNextPage && !isFetchingNextPage) {
                fetchNextPage();
            }
        },
        [fetchNextPage, hasNextPage, isFetchingNextPage],
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
        <div className="space-y-4">
            <h1 className="text-2xl font-bold text-gray-900">Metadata</h1>
            {MAP_TILE_ATTRIBUTION && (
                <p className="text-xs text-gray-400">
                    Map: {attributionToText(MAP_TILE_ATTRIBUTION)}
                </p>
            )}

            {/* ── Filter bar ─────────────────────────────────── */}
            <div className="flex flex-wrap items-end gap-3 rounded-lg border border-gray-200 bg-white p-3">
                <label className="flex flex-col gap-1 text-xs font-medium text-gray-600">
                    VIN pattern
                    <input
                        type="text"
                        placeholder="99?? or 1*"
                        value={vinPattern}
                        onChange={(e) => setVinPattern(e.target.value)}
                        className="w-28 rounded border border-gray-300 px-2 py-1 text-sm"
                    />
                </label>
                <label className="flex flex-col gap-1 text-xs font-medium text-gray-600">
                    Model
                    <input
                        type="text"
                        placeholder="DEFAULT* or ?EH*"
                        value={modelFilter}
                        onChange={(e) => setModelFilter(e.target.value)}
                        className="w-40 rounded border border-gray-300 px-2 py-1 text-sm"
                    />
                </label>
                <DateTimePicker label="From" value={timeStart} onChange={setTimeStart} />
                <DateTimePicker label="To" value={timeEnd} onChange={setTimeEnd} />
                <div className="flex flex-col gap-1 text-xs font-medium text-gray-600">
                    Geo bounds
                    <div className="flex items-center gap-1">
                        <button
                            onClick={() => setShowMapPicker(true)}
                            className="flex items-center gap-1 rounded border border-gray-300 px-2 py-1 text-sm hover:bg-gray-50"
                        >
                            <MapPin className="h-3.5 w-3.5" />
                            {geoBounds ? "Change" : "Select"}
                        </button>
                        {geoBounds && (
                            <button
                                onClick={() => setGeoBounds(null)}
                                className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
                            >
                                <X className="h-3.5 w-3.5" />
                            </button>
                        )}
                    </div>
                </div>
                {geoBounds && (
                    <span className="self-end text-xs text-gray-500">
                        ({geoBounds.min_lat.toFixed(3)},{geoBounds.min_lng.toFixed(3)}) –
                        ({geoBounds.max_lat.toFixed(3)},{geoBounds.max_lng.toFixed(3)})
                    </span>
                )}
            </div>

            {/* ── Map picker modal ───────────────────────────── */}
            {showMapPicker && (
                <RectanglePicker
                    initial={geoBounds}
                    onConfirm={(b) => {
                        setGeoBounds(b);
                        setShowMapPicker(false);
                    }}
                    onCancel={() => setShowMapPicker(false)}
                />
            )}

            {/* ── Results ────────────────────────────────────── */}
            {isLoading && (
                <p className="text-sm text-gray-500">Loading...</p>
            )}
            {error && (
                <p className="text-sm text-red-600">
                    Failed to load metadata: {String(error)}
                </p>
            )}

            {!isLoading && items.length === 0 && (
                <p className="text-sm text-gray-500">No metadata records found.</p>
            )}

            <div className="grid gap-4">
                {items.map((item) => (
                    <div
                        key={`${item.vin}-${item.start_time}`}
                        onClick={() =>
                            navigate({
                                to: "/metadata/$vin/$startTime",
                                params: {
                                    vin: item.vin,
                                    startTime: item.start_time,
                                },
                            })
                        }
                        className="flex cursor-pointer gap-4 rounded-lg border border-gray-200 bg-white p-4 shadow-sm transition-colors hover:border-indigo-300 hover:bg-indigo-50/30"
                    >
                        {/* Mini map */}
                        <div className="h-32 w-48 flex-shrink-0 overflow-hidden rounded-md border border-gray-200">
                            <MiniMapThumbnail item={item} />
                        </div>

                        {/* First camera image */}
                        <div className="h-32 w-32 flex-shrink-0 overflow-hidden rounded-md border border-gray-200">
                            {item.first_image_url ? (
                                <img
                                    src={item.first_image_url}
                                    alt="First frame"
                                    className="h-full w-full object-cover"
                                    loading="lazy"
                                />
                            ) : (
                                <div className="flex h-full items-center justify-center bg-gray-100 text-xs text-gray-400">
                                    No image
                                </div>
                            )}
                        </div>

                        {/* Info */}
                        <div className="flex min-w-0 flex-1 flex-col justify-center gap-1">
                            <p className="truncate text-sm font-semibold text-gray-900">
                                {item.vin}
                            </p>
                            <p className="text-xs text-gray-500">
                                Model: {item.model}
                            </p>
                            <p className="text-xs text-gray-500">
                                {formatDateTime(item.start_time)} — {formatDateTime(item.end_time)}
                            </p>
                            <div className="mt-1 flex gap-4 text-xs text-gray-600">
                                <span>
                                    Trajectory: {item.trajectory_length} pts
                                </span>
                                <span>
                                    Image sets: {item.image_set_count}
                                </span>
                            </div>
                            {item.active_behaviors.length > 0 && (
                                <div className="mt-1.5 flex flex-wrap gap-1">
                                    {item.active_behaviors.map((ab, i) => (
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
                    </div>
                ))}
            </div>

            {/* Infinite scroll sentinel */}
            <div ref={sentinelRef} className="h-4" />
            {isFetchingNextPage && (
                <p className="text-center text-sm text-gray-500">Loading more...</p>
            )}
        </div>
    );
}
