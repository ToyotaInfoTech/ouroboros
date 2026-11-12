// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { describe, it, expect } from "vitest";
import { cn, formatTimestamp, getTagColor, unescapeNewlines } from "./utils";

describe("cn", () => {
    it("merges class names", () => {
        const result = cn("px-2", "py-1");
        expect(result).toBe("px-2 py-1");
    });

    it("deduplicates tailwind classes", () => {
        const result = cn("px-2", "px-4");
        expect(result).toBe("px-4");
    });

    it("handles conditional classes", () => {
        const condition = false;
        const result = cn("base", condition && "hidden", "text-sm");
        expect(result).toBe("base text-sm");
    });

    it("handles empty input", () => {
        expect(cn()).toBe("");
    });
});

describe("formatTimestamp", () => {
    it("returns a non-empty string for a valid timestamp", () => {
        const result = formatTimestamp(1700000000000);
        expect(result).toBeTruthy();
        expect(typeof result).toBe("string");
    });

    it("returns a string for epoch zero", () => {
        const result = formatTimestamp(0);
        expect(result).toBeTruthy();
    });
});

describe("getTagColor", () => {
    it("returns an object with bg and text", () => {
        const color = getTagColor("test-tag");
        expect(color).toHaveProperty("bg");
        expect(color).toHaveProperty("text");
        expect(color.bg).toMatch(/^#/);
        expect(color.text).toMatch(/^#/);
    });

    it("returns same color for same input (deterministic)", () => {
        const a = getTagColor("my-tag");
        const b = getTagColor("my-tag");
        expect(a).toEqual(b);
    });

    it("returns different colors for different inputs (usually)", () => {
        const a = getTagColor("alpha");
        const b = getTagColor("beta");
        // Not guaranteed to differ, but extremely likely with these specific inputs
        expect(a.bg !== b.bg || a.text !== b.text).toBe(true);
    });

    it("handles empty string", () => {
        const color = getTagColor("");
        expect(color).toHaveProperty("bg");
    });
});

describe("unescapeNewlines", () => {
    it("unescapes \\n in quoted strings", () => {
        const input = '{"sql": "SELECT\\nFROM t"}';
        const result = unescapeNewlines(input);
        expect(result).toContain("\n");
    });

    it("leaves strings without newlines unchanged", () => {
        const input = '{"key": "value"}';
        const result = unescapeNewlines(input);
        expect(result).toBe(input);
    });

    it("handles malformed JSON strings gracefully", () => {
        const input = '"unterminated';
        // Should not throw
        const result = unescapeNewlines(input);
        expect(typeof result).toBe("string");
    });

    it("handles empty input", () => {
        expect(unescapeNewlines("")).toBe("");
    });
});
