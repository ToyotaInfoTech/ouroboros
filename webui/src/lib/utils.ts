// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
    return twMerge(clsx(inputs))
}

/** Format a Unix millisecond timestamp as a locale-aware local time string. */
export function formatTimestamp(ms: number): string {
    return new Date(ms).toLocaleString();
}

/**
 * Color map entry for tag background styling.
 */
export interface TagColor {
    bg: string;
    text: string;
}

const TAG_COLOR_MAP: TagColor[] = [
    { bg: "#fce4ec", text: "#880e4f" }, // Rose
    { bg: "#fff3e0", text: "#bf360c" }, // Orange
    { bg: "#fff8e1", text: "#f57f17" }, // Amber
    { bg: "#f1f8e9", text: "#33691e" }, // Lime
    { bg: "#e8f5e9", text: "#1b5e20" }, // Green
    { bg: "#e0f2f1", text: "#004d40" }, // Teal
    { bg: "#e0f7fa", text: "#006064" }, // Cyan
    { bg: "#e3f2fd", text: "#0d47a1" }, // Blue
    { bg: "#e8eaf6", text: "#1a237e" }, // Indigo
    { bg: "#f3e5f5", text: "#4a148c" }, // Purple
    { bg: "#ede7f6", text: "#311b92" }, // Deep Purple
    { bg: "#efebe9", text: "#3e2723" }, // Brown
];

/** djb2 hash for a string, returning a non-negative integer. */
function hashString(s: string): number {
    let hash = 5381;
    for (let i = 0; i < s.length; i++) {
        hash = ((hash << 5) + hash + s.charCodeAt(i)) | 0;
    }
    return Math.abs(hash);
}

/**
 * Return a consistent background / text color pair for a tag id.
 * Uses `hash(id) % colorMapSize` so the same id always maps to the same color.
 */
export function getTagColor(id: string): TagColor {
    const idx = hashString(id) % TAG_COLOR_MAP.length;
    return TAG_COLOR_MAP[idx];
}

/**
 * Unescape `\n` and `\t` sequences inside quoted string literals in
 * JSON-formatted text so that multi-line values (such as SQL in `init_sql`)
 * render readably in a `<pre>` element.
 *
 * Uses JSON.parse on each matched string literal to handle all escape
 * sequences correctly, including the `\\n` (literal backslash-n) vs `\n`
 * (actual newline) distinction.
 *
 * For display only — the result is not valid JSON for copy-paste.
 * The original (escaped) text should be used for clipboard operations.
 */
export function unescapeNewlines(text: string): string {
    return text.replace(/"(?:[^"\\]|\\.)*"/g, (match) => {
        try {
            const value = JSON.parse(match) as string;
            if (typeof value !== "string" || !value.includes("\n")) return match;
            return (
                '"' +
                value.replace(/\\/g, "\\\\").replace(/"/g, '\\"') +
                '"'
            );
        } catch {
            return match;
        }
    });
}
