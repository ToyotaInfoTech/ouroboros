// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


// HTTP API client for the daemon REST endpoints.

const BASE_URL = "/api/v1";
const TOKEN_KEY = "ouro_webui_token";

/** Read the access token kept for this browser session, if any. */
export function getAuthToken(): string | null {
    return sessionStorage.getItem(TOKEN_KEY);
}

let pendingToken: Promise<boolean> | null = null;

/** Ask for the access token once, even when several requests fail at the same time. */
function promptForToken(): Promise<boolean> {
    pendingToken ??= Promise.resolve().then(() => {
        const token = window.prompt("Enter the WebUI access token");
        pendingToken = null;
        if (!token) return false;
        sessionStorage.setItem(TOKEN_KEY, token.trim());
        return true;
    });
    return pendingToken;
}

async function request<T>(
    path: string,
    options?: RequestInit,
    retried = false,
): Promise<T> {
    const token = getAuthToken();
    const res = await fetch(`${BASE_URL}${path}`, {
        ...options,
        headers: {
            "Content-Type": "application/json",
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
            ...options?.headers,
        },
    });
    if (res.status === 401) {
        sessionStorage.removeItem(TOKEN_KEY);
        if (!retried && (await promptForToken())) {
            return request<T>(path, options, true);
        }
    }
    if (!res.ok) {
        const text = await res.text();
        throw new Error(`${res.status}: ${text}`);
    }
    if (res.status === 204) return undefined as T;
    return res.json();
}

// --- Shadows ---

import type {
    StateTag,
    EdgeShadow,
    AssignedTag,
    TagBody,
    TagAssignBody,
    CommandKind,
    AllTagAssignments,
    BatchCommandRequest,
    BatchCommandResponse,
} from "./types";

export const shadows = {
    list: () => request<Record<string, EdgeShadow>>("/shadows"),
    get: (id: string) => request<EdgeShadow | null>(`/shadows/${encodeURIComponent(id)}`),
    delete: (id: string) =>
        request<void>(`/shadows/${encodeURIComponent(id)}`, { method: "DELETE" }),
};

// --- Tags ---

export const tags = {
    list: () => request<StateTag[]>("/tags"),
    get: (id: string) => request<StateTag>(`/tags/${encodeURIComponent(id)}`),
    create: (body: TagBody) =>
        request<void>("/tags", { method: "POST", body: JSON.stringify(body) }),
    update: (id: string, body: TagBody) =>
        request<void>(`/tags/${encodeURIComponent(id)}`, {
            method: "PUT",
            body: JSON.stringify(body),
        }),
    delete: (id: string) =>
        request<void>(`/tags/${encodeURIComponent(id)}`, { method: "DELETE" }),
    assign: (body: TagAssignBody) =>
        request<void>("/tags/assign", {
            method: "POST",
            body: JSON.stringify(body),
        }),
    unassign: (body: TagAssignBody) =>
        request<void>("/tags/assign", {
            method: "DELETE",
            body: JSON.stringify(body),
        }),
};

// --- Edges ---

export const edges = {
    getTags: (edgeId: string) =>
        request<AssignedTag[]>(`/edges/${encodeURIComponent(edgeId)}/tags`),
    getAllTags: () => request<AllTagAssignments>("/edges/tags"),
    previewConfig: (edgeId: string) =>
        request<Record<string, unknown>>(
            `/edges/${encodeURIComponent(edgeId)}/preview`,
        ),
    sendCommand: (edgeId: string, kind: CommandKind) =>
        request<void>(`/edges/${encodeURIComponent(edgeId)}/command`, {
            method: "POST",
            body: JSON.stringify({ kind }),
        }),
    batchCommand: (body: BatchCommandRequest) =>
        request<BatchCommandResponse>("/edges/batch/command", {
            method: "POST",
            body: JSON.stringify(body),
        }),
};

// --- Health ---

export const system = {
    health: () => request<void>("/health"),
};

// --- Metadata ---

import type { MetadataListItem, MetadataDetail, MetadataListParams } from "./types";

export const metadata = {
    list: (params?: MetadataListParams) => {
        const query = new URLSearchParams();
        if (params) {
            for (const [key, value] of Object.entries(params)) {
                if (value !== undefined) query.set(key, String(value));
            }
        }
        const qs = query.toString();
        return request<MetadataListItem[]>(`/metadata${qs ? `?${qs}` : ""}`);
    },
    get: (vin: string, startTime: string) =>
        request<MetadataDetail>(
            `/metadata/${encodeURIComponent(vin)}/${encodeURIComponent(startTime)}`,
        ),
};