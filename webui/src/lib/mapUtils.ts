// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

import type { StyleSpecification, LngLatBoundsLike } from "maplibre-gl";
import type { BoundingBox } from "../api/types";
import type { GeoBounds } from "../stores/metadataFilterStore";

/** Raster tile URL template (`{z}/{x}/{y}`) supplied at build time; empty means no basemap. */
export const MAP_TILE_URL: string = (import.meta.env.VITE_MAP_TILE_URL ?? "").trim();

/** Attribution required by the configured tile provider (HTML allowed); empty if none. */
export const MAP_TILE_ATTRIBUTION: string = (import.meta.env.VITE_MAP_ATTRIBUTION ?? "").trim();

/** Background colour used where no tile is drawn. */
export const MAP_BACKGROUND_COLOR = "#e2e8f0";

/** Fill a tile URL template with tile coordinates. */
export function tileImageUrl(template: string, z: number, x: number, y: number): string {
    return template
        .replaceAll("{z}", String(z))
        .replaceAll("{x}", String(x))
        .replaceAll("{y}", String(y));
}

/** Strip markup from an attribution string so it can be shown as plain text. */
export function attributionToText(attribution: string): string {
    return attribution.replace(/<[^>]*>/g, "").trim();
}

/** Build a maplibre-gl style for the map pages.
 *
 * Without a tile URL the style is just a flat background, so no third-party
 * tile server is contacted.  With a tile URL it contains a single raster layer.
 *
 * Note: glyphs and sprite are intentionally omitted because this style has no
 * symbol/text layers that would need glyph fonts or icon sprites.
 */
export function buildMapStyle(tileUrl: string, attribution: string): StyleSpecification {
    if (!tileUrl) {
        return {
            version: 8,
            sources: {},
            layers: [
                {
                    id: "background",
                    type: "background",
                    paint: { "background-color": MAP_BACKGROUND_COLOR },
                },
            ],
        };
    }
    return {
        version: 8,
        sources: {
            basemap: {
                type: "raster",
                tiles: [tileUrl],
                tileSize: 256,
                ...(attribution ? { attribution } : {}),
                maxzoom: 19,
            },
        },
        layers: [
            {
                id: "background",
                type: "background",
                paint: { "background-color": MAP_BACKGROUND_COLOR },
            },
            { id: "basemap-tiles", type: "raster", source: "basemap" },
        ],
    };
}

/** Map style built from the build-time tile configuration. */
export const MAP_STYLE: StyleSpecification = buildMapStyle(MAP_TILE_URL, MAP_TILE_ATTRIBUTION);

/** Convert a BoundingBox to maplibre LngLatBoundsLike [west, south, east, north]. */
export function bboxToLngLatBounds(bbox: BoundingBox): LngLatBoundsLike {
    return [bbox.min_lng, bbox.min_lat, bbox.max_lng, bbox.max_lat];
}

/** Convert a GeoBounds to maplibre LngLatBoundsLike [west, south, east, north]. */
export function geoBoundsToLngLatBounds(bounds: GeoBounds): LngLatBoundsLike {
    return [bounds.min_lng, bounds.min_lat, bounds.max_lng, bounds.max_lat];
}

/** Convert [minLng, minLat, maxLng, maxLat] to a GeoJSON Polygon FeatureCollection. */
export function boundsToGeoJSON(
    minLng: number,
    minLat: number,
    maxLng: number,
    maxLat: number,
): GeoJSON.FeatureCollection {
    return {
        type: "FeatureCollection",
        features: [
            {
                type: "Feature",
                geometry: {
                    type: "Polygon",
                    coordinates: [
                        [
                            [minLng, minLat],
                            [maxLng, minLat],
                            [maxLng, maxLat],
                            [minLng, maxLat],
                            [minLng, minLat],
                        ],
                    ],
                },
                properties: {},
            },
        ],
    };
}

/** Convert a BoundingBox to a GeoJSON Polygon FeatureCollection. */
export function bboxToGeoJSON(bbox: BoundingBox): GeoJSON.FeatureCollection {
    return boundsToGeoJSON(bbox.min_lng, bbox.min_lat, bbox.max_lng, bbox.max_lat);
}
