// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { describe, it, expect, vi, beforeEach } from "vitest";
import { shadows, tags, edges, system } from "./client";

// Mock global fetch
const mockFetch = vi.fn();
vi.stubGlobal("fetch", mockFetch);

beforeEach(() => {
    mockFetch.mockReset();
});

function mockJsonResponse(data: unknown, status = 200) {
    mockFetch.mockResolvedValueOnce({
        ok: true,
        status,
        json: () => Promise.resolve(data),
        text: () => Promise.resolve(JSON.stringify(data)),
    });
}

function mock204Response() {
    mockFetch.mockResolvedValueOnce({
        ok: true,
        status: 204,
        json: () => Promise.resolve(undefined),
        text: () => Promise.resolve(""),
    });
}

function mockErrorResponse(status: number, text: string) {
    mockFetch.mockResolvedValueOnce({
        ok: false,
        status,
        text: () => Promise.resolve(text),
    });
}

describe("shadows", () => {
    it("list fetches /api/v1/shadows", async () => {
        const data = { edge1: { edge_id: "edge1" } };
        mockJsonResponse(data);
        const result = await shadows.list();
        expect(result).toEqual(data);
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/shadows",
            expect.objectContaining({ headers: { "Content-Type": "application/json" } }),
        );
    });

    it("get fetches a specific shadow", async () => {
        const data = { edge_id: "edge1" };
        mockJsonResponse(data);
        const result = await shadows.get("edge1");
        expect(result).toEqual(data);
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/shadows/edge1",
            expect.anything(),
        );
    });

    it("get encodes special characters in id", async () => {
        mockJsonResponse(null);
        await shadows.get("edge/special");
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/shadows/edge%2Fspecial",
            expect.anything(),
        );
    });

    it("delete sends DELETE method", async () => {
        mock204Response();
        await shadows.delete("edge1");
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/shadows/edge1",
            expect.objectContaining({ method: "DELETE" }),
        );
    });
});

describe("tags", () => {
    it("list fetches /api/v1/tags", async () => {
        const data = [{ tag_id: "tag1" }];
        mockJsonResponse(data);
        const result = await tags.list();
        expect(result).toEqual(data);
    });

    it("create sends POST with body", async () => {
        mock204Response();
        await tags.create({ id: "new-tag", priority: 0, patch: {} });
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/tags",
            expect.objectContaining({
                method: "POST",
                body: expect.any(String),
            }),
        );
    });

    it("update sends PUT with body", async () => {
        mock204Response();
        await tags.update("tag1", { id: "tag1", priority: 0, patch: {} });
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/tags/tag1",
            expect.objectContaining({
                method: "PUT",
                body: expect.any(String),
            }),
        );
    });

    it("delete sends DELETE method", async () => {
        mock204Response();
        await tags.delete("tag1");
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/tags/tag1",
            expect.objectContaining({ method: "DELETE" }),
        );
    });
});

describe("edges", () => {
    it("sendCommand sends POST with kind", async () => {
        mock204Response();
        await edges.sendCommand("edge1", "shutdown");
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/edges/edge1/command",
            expect.objectContaining({
                method: "POST",
                body: JSON.stringify({ kind: "shutdown" }),
            }),
        );
    });

    it("batchCommand sends POST with edge_ids and kind", async () => {
        const resp = { succeeded: ["e1"], failed: [] };
        mockJsonResponse(resp);
        const result = await edges.batchCommand({
            edge_ids: ["e1"],
            kind: "restart",
        });
        expect(result).toEqual(resp);
    });
});

describe("error handling", () => {
    it("throws on non-ok response", async () => {
        mockErrorResponse(404, "Not found");
        await expect(shadows.get("nonexistent")).rejects.toThrow("404: Not found");
    });

    it("throws on 500 error", async () => {
        mockErrorResponse(500, "Internal server error");
        await expect(shadows.list()).rejects.toThrow("500: Internal server error");
    });
});

describe("system", () => {
    it("health fetches /api/v1/health", async () => {
        mock204Response();
        await system.health();
        expect(mockFetch).toHaveBeenCalledWith(
            "/api/v1/health",
            expect.anything(),
        );
    });
});
