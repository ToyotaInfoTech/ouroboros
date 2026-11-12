// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


// API type definitions corresponding to Rust types in ouro_schema/ouro_state.

/** Edge device status. */
export type EdgeStatus = "Up" | "Error" | "Starting" | "Stopping" | "Down" | "Suspended";

/** CPU information from sysinfo. */
export interface CpuInfo {
    name: string;
    usage: number;
    frequency_mhz: number;
    brand: string;
    vendor_id: string;
}

/** System load averages. */
export interface LoadAverage {
    one: number;
    five: number;
    fifteen: number;
}

/** System information reported by an edge. */
export interface SystemInfo {
    global_cpu_usage: number;
    cpus: CpuInfo[];
    total_memory: number;
    used_memory: number;
    available_memory: number;
    total_swap: number;
    used_swap: number;
    uptime: number;
    load_average: LoadAverage;
    os_name?: string;
    kernel_version?: string;
    os_version?: string;
    host_name?: string;
}

/** Tag entry within a state epoch. */
export interface EpochTag {
    id: string;
    epoch: number;
    display_name?: string;
    priority?: number;
}

/** Metadata describing schema version and tag epoch for a state. */
export interface StateMetadata {
    schema_version: number;
    epoch: EpochTag[];
}

/** Task status reported by an edge device. */
export type TaskStatus = "Created" | { Active: number } | { Inactive: number } | "Discarded";

/** Individual task state within an edge report. */
export interface TaskState {
    name: string;
    status: TaskStatus;
}

/** State report from an edge device. */
export interface EdgeStateReport {
    edge_id: string;
    place_id?: string;
    current_state: StateMetadata;
    desired_state?: StateMetadata;
    status: EdgeStatus;
    system_info?: SystemInfo;
    timestamp: number;
    task_states?: TaskState[];
}

/** Edge shadow: desired + reported state. */
export interface EdgeShadow {
    edge_id: string;
    last_report?: EdgeStateReport;
    desired_state?: { metadata: StateMetadata;[key: string]: unknown };
    last_seen?: number;
}

/** Configuration tag with a merge patch. */
export interface StateTag {
    id: string;
    display_name?: string;
    priority: number;
    patch: Record<string, unknown>;
    created_at?: string;
    updated_at?: string;
}

/** Tag assigned to an edge with resolved priority. */
export interface AssignedTag {
    id: string;
    priority: number;
    patch: Record<string, unknown>;
}

/** Request body for creating/updating a tag. */
export interface TagBody {
    id?: string;
    display_name?: string;
    priority: number;
    patch: Record<string, unknown>;
}

/** Request body for tag assignment. */
export interface TagAssignBody {
    edge_id: string;
    id: string;
}

/** WebSocket dashboard event. */
export type DashboardEvent =
    | { type: "EdgeStateChanged"; data: { edge_id: string; status: string } }
    | { type: "ShadowUpdated"; data: { edge_id: string } }
    | { type: "ShadowDeleted"; data: { edge_id: string } }
    | { type: "TagChanged"; data: { id: string; action: string } }
    | {
        type: "TagAssignmentChanged";
        data: { edge_id: string; id: string; action: string };
    };

/** Command kind for edge lifecycle operations. */
export type CommandKind = "start" | "shutdown" | "restart" | "suspend";

/** All edge-tag assignments keyed by edge ID. */
export type AllTagAssignments = Record<string, AssignedTag[]>;

/** Request body for batch command. */
export interface BatchCommandRequest {
    edge_ids: string[];
    kind: CommandKind;
}

/** Response from batch command. */
export interface BatchCommandResponse {
    succeeded: string[];
    failed: { edge_id: string; error: string }[];
}

// ── Metadata types ─────────────────────────────────────────────────────

/** Geographic bounding box for a trajectory. */
export interface BoundingBox {
    min_lat: number;
    max_lat: number;
    min_lng: number;
    max_lng: number;
}

/** Summary item returned in the metadata list. */
export interface MetadataListItem {
    vin: string;
    start_time: string;
    end_time: string;
    model: string;
    trajectory_length: number;
    image_set_count: number;
    bbox: BoundingBox | null;
    first_image_url: string | null;
    /** Full trajectory for thumbnail rendering (same type as detail). */
    trajectory: LocationFix[];
    /** Behavior activation spans observed during this metadata window. */
    active_behaviors: ActiveBehavior[];
}

/** A behavior activation span within a metadata window. */
export interface ActiveBehavior {
    name: string;
    start_time: string;
    end_time: string;
}

/** Single location fix in a trajectory. */
export interface LocationFix {
    timestamp: string;
    latitude: number;
    longitude: number;
}

/** Reference to a camera image (without pixel data). */
export interface CameraFrameRef {
    camera_id: string;
    ts: string;
    image_url: string;
}

/** A behavior activation span with RFC 3339 timestamps. */
export interface ActiveBehavior {
    name: string;
    start_time: string;
    end_time: string;
}

/** Full metadata detail including trajectory and camera frames. */
export interface MetadataDetail {
    vin: string;
    start_time: string;
    end_time: string;
    model: string;
    trajectory: LocationFix[];
    camera_frames: CameraFrameRef[];
    /** Behavior activation spans observed during this metadata window. */
    active_behaviors: ActiveBehavior[];
}

/** Query parameters for the metadata list endpoint. */
export interface MetadataListParams {
    limit?: number;
    offset?: number;
    vin_pattern?: string;
    model?: string;
    time_start?: string;
    time_end?: string;
    min_lat?: number;
    max_lat?: number;
    min_lng?: number;
    max_lng?: number;
}
