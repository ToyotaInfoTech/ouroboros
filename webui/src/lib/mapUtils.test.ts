// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from "vitest";
import {
    MAP_BACKGROUND_COLOR,
    attributionToText,
    buildMapStyle,
    tileImageUrl,
} from "./mapUtils";

describe("buildMapStyle", () => {
    it("has no sources when no tile URL is given", () => {
        const style = buildMapStyle("", "");
        expect(style.sources).toEqual({});
        expect(style.layers).toEqual([
            {
                id: "background",
                type: "background",
                paint: { "background-color": MAP_BACKGROUND_COLOR },
            },
        ]);
    });

    it("adds a raster layer for the configured tile URL", () => {
        const url = "https://tiles.example.com/{z}/{x}/{y}.png";
        const style = buildMapStyle(url, "Example Maps");
        expect(style.sources.basemap).toMatchObject({
            type: "raster",
            tiles: [url],
            attribution: "Example Maps",
        });
        expect(style.layers.map((l) => l.id)).toEqual(["background", "basemap-tiles"]);
    });

    it("omits the attribution when none is given", () => {
        const style = buildMapStyle("https://tiles.example.com/{z}/{x}/{y}.png", "");
        expect(style.sources.basemap).not.toHaveProperty("attribution");
    });
});

describe("tileImageUrl", () => {
    it("fills in the tile coordinates", () => {
        expect(tileImageUrl("https://t.example.com/{z}/{x}/{y}.png", 12, 3, 4)).toBe(
            "https://t.example.com/12/3/4.png",
        );
    });

    it("replaces every occurrence of a placeholder", () => {
        expect(tileImageUrl("/{z}/{z}/{x}/{y}", 5, 6, 7)).toBe("/5/5/6/7");
    });
});

describe("attributionToText", () => {
    it("strips markup", () => {
        expect(attributionToText('© <a href="https://example.com">Example</a> maps')).toBe(
            "© Example maps",
        );
    });

    it("returns plain text unchanged", () => {
        expect(attributionToText("Plain text")).toBe("Plain text");
    });
});
