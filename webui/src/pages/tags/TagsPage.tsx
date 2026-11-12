// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { tags } from "../../api/client";
import type { StateTag, TagBody } from "../../api/types";
import { useCallback, useEffect, useRef, useState } from "react";
import { parse as tomlParse, stringify as tomlStringify } from "smol-toml";
import { formatTimestamp, getTagColor, unescapeNewlines } from "../../lib/utils";
import { TagId } from "../../components/ui/TagId";
import { toast } from "../../stores/toastStore";
import { GripHorizontal } from "lucide-react";

/**
 * Serialize an object to TOML, converting strings that contain newlines to
 * triple-quoted multi-line TOML basic strings so that multi-line values
 * (such as SQL in `init_sql`) are editable naturally in a textarea.
 *
 * The result is valid TOML and round-trips correctly through tomlParse.
 */
function tomlStringifyMultiline(obj: Record<string, unknown>): string {
    const raw = tomlStringify(obj);
    return raw.replace(/"(?:[^"\\]|\\.)*"/g, (match) => {
        try {
            const value = JSON.parse(match) as string;
            if (typeof value !== "string" || !value.includes("\n")) return match;
            // TOML spec trims the first newline after the opening """,
            // so adding one here ensures content always starts on a fresh line.
            return `"""\n${value.replace(/"""/g, '\\"""')}"""`;
        } catch {
            return match;
        }
    });
}

/**
 * For textarea display: convert \n (and \t) escape sequences within JSON
 * string literals to actual characters, so multi-line values (such as SQL
 * in `init_sql`) appear naturally in the editor.
 */
function jsonUnescapeForEdit(json: string): string {
    return json.replace(/"(?:[^"\\]|\\.)*"/g, (match) => {
        // match.slice strips surrounding quotes; inner is the raw escape sequence text
        const inner = match.slice(1, -1)
            .replace(/\\n/g, "\n")
            .replace(/\\t/g, "\t")
            .replace(/\\r/g, "");
        return '"' + inner + '"';
    });
}

/**
 * Re-escape actual newlines / tabs within JSON string literals before
 * calling JSON.parse, to round-trip from the display format back to
 * valid JSON.
 */
function jsonReescapeForParse(text: string): string {
    return text.replace(/"(?:[^"\\]|\\.)*"/g, (match) => {
        const inner = match.slice(1, -1)
            .replace(/\r\n/g, "\\n")
            .replace(/\r/g, "\\n")
            .replace(/\n/g, "\\n")
            .replace(/\t/g, "\\t");
        return '"' + inner + '"';
    });
}

export type PatchFormat = "json" | "toml";

export function TagsPage({ initialSelectedTagId }: { initialSelectedTagId?: string } = {}) {
    const queryClient = useQueryClient();
    const [showForm, setShowForm] = useState(false);
    const [editingTag, setEditingTag] = useState<StateTag | null>(null);
    const [selectedTag, setSelectedTag] = useState<StateTag | null>(null);

    const { data: tagList, isLoading, error } = useQuery({
        queryKey: ["tags"],
        queryFn: tags.list,
    });

    // Pre-select the tag when navigating from /tags/$tagId
    useEffect(() => {
        if (!initialSelectedTagId || !tagList) return;
        const found = tagList.find((t) => t.id === initialSelectedTagId) ?? null;
        // eslint-disable-next-line react-hooks/set-state-in-effect
        setSelectedTag(found);
        setShowForm(false);
    }, [initialSelectedTagId, tagList]);

    const deleteMutation = useMutation({
        mutationFn: (id: string) => tags.delete(id),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ["tags"] }),
        onError: (err) => toast.error(`Failed to delete tag: ${String(err)}`),
    });

    const createMutation = useMutation({
        mutationFn: (body: TagBody) => tags.create(body),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ["tags"] });
            setShowForm(false);
        },
        onError: (err) => toast.error(`Failed to create tag: ${String(err)}`),
    });

    const updateMutation = useMutation({
        mutationFn: ({ id, body }: { id: string; body: TagBody }) =>
            tags.update(id, body),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ["tags"] });
            setShowForm(false);
            setEditingTag(null);
        },
        onError: (err) => toast.error(`Failed to update tag: ${String(err)}`),
    });

    return (
        <div className="space-y-6">
            <div className="flex items-center justify-between">
                <h1 className="text-2xl font-bold text-gray-900">Tags</h1>
                <button
                    onClick={() => {
                        setEditingTag(null);
                        setShowForm(true);
                    }}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
                >
                    Create Tag
                </button>
            </div>

            {showForm && (
                <TagForm
                    tag={editingTag}
                    onSubmit={(body) => {
                        if (editingTag) {
                            updateMutation.mutate({ id: editingTag.id, body });
                        } else {
                            createMutation.mutate(body);
                        }
                    }}
                    onCancel={() => {
                        setShowForm(false);
                        setEditingTag(null);
                    }}
                    isPending={createMutation.isPending || updateMutation.isPending}
                />
            )}

            {selectedTag && !showForm && (
                <TagDetailPanel
                    tag={selectedTag}
                    onClose={() => setSelectedTag(null)}
                    onEdit={() => {
                        setEditingTag(selectedTag);
                        setSelectedTag(null);
                        setShowForm(true);
                    }}
                />
            )}

            {isLoading ? (
                <p className="text-sm text-gray-500">Loading...</p>
            ) : error ? (
                <p className="text-sm text-red-600">
                    Failed to load tags: {String(error)}
                </p>
            ) : (
                <div className="overflow-hidden rounded-lg border border-gray-200">
                    <table className="min-w-full divide-y divide-gray-200">
                        <thead className="bg-gray-50">
                            <tr>
                                <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                                    Name
                                </th>
                                <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                                    ID
                                </th>
                                <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">
                                    Priority
                                </th>
                                <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">
                                    Actions
                                </th>
                            </tr>
                        </thead>
                        <tbody className="divide-y divide-gray-200 bg-white">
                            {!tagList || tagList.length === 0 ? (
                                <tr>
                                    <td
                                        colSpan={4}
                                        className="px-4 py-8 text-center text-sm text-gray-400"
                                    >
                                        No tags defined
                                    </td>
                                </tr>
                            ) : (
                                tagList.map((tag) => (
                                    <tr key={tag.id} className="hover:bg-gray-50">
                                        <td className="px-4 py-3 text-sm font-medium">
                                            {(() => {
                                                const tc = getTagColor(tag.id);
                                                return (
                                                    <button
                                                        type="button"
                                                        onClick={() => {
                                                            setShowForm(false);
                                                            setSelectedTag((prev) =>
                                                                prev?.id === tag.id ? null : tag
                                                            );
                                                        }}
                                                        className="inline-block rounded px-2 py-0.5 hover:opacity-80"
                                                        style={{ backgroundColor: tc.bg, color: tc.text }}
                                                    >
                                                        {tag.display_name ?? "—"}
                                                    </button>
                                                );
                                            })()}
                                        </td>
                                        <td className="px-4 py-3 text-sm text-gray-500">
                                            <TagId id={tag.id} />
                                        </td>
                                        <td className="px-4 py-3 text-sm text-gray-500">
                                            {tag.priority}
                                        </td>
                                        <td className="px-4 py-3 text-right text-sm">
                                            <button
                                                onClick={() => {
                                                    setEditingTag(tag);
                                                    setShowForm(true);
                                                }}
                                                className="mr-2 text-indigo-600 hover:underline"
                                            >
                                                Edit
                                            </button>
                                            <button
                                                onClick={() => {
                                                    if (confirm(`Delete tag "${tag.id}"?`)) {
                                                        deleteMutation.mutate(tag.id);
                                                    }
                                                }}
                                                className="text-red-600 hover:underline"
                                                disabled={deleteMutation.isPending}
                                            >
                                                Delete
                                            </button>
                                        </td>
                                    </tr>
                                ))
                            )}
                        </tbody>
                    </table>
                </div>
            )}
        </div>
    );
}

export function TagForm({
    tag,
    onSubmit,
    onCancel,
    isPending,
}: {
    tag: StateTag | null;
    onSubmit: (body: TagBody) => void;
    onCancel: () => void;
    isPending: boolean;
}) {
    const [displayName, setDisplayName] = useState(tag?.display_name ?? "");
    const [priority, setPriority] = useState(tag?.priority ?? 0);
    const [patchText, setPatchText] = useState(
        tag ? jsonUnescapeForEdit(JSON.stringify(tag.patch, null, 2)) : "{}",
    );
    const [patchFormat, setPatchFormat] = useState<PatchFormat>("json");
    const [syntaxError, setSyntaxError] = useState<string | null>(null);
    const [copied, setCopied] = useState(false);

    const handleCopy = useCallback(() => {
        if (!patchText) return;
        // In JSON mode the textarea contains actual newlines for readability;
        // re-escape them so the clipboard holds valid JSON.
        const copyText =
            patchFormat === "json" ? jsonReescapeForParse(patchText) : patchText;
        navigator.clipboard.writeText(copyText).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        }).catch(() => { /* clipboard permission denied */ });
    }, [patchText, patchFormat]);

    const parsePatchText = useCallback(
        (text: string, fmt: PatchFormat): Record<string, unknown> => {
            if (fmt === "toml") {
                const parsed = tomlParse(text);
                if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
                    throw new Error("TOML must parse to an object");
                }
                return parsed as Record<string, unknown>;
            }
            const parsed = JSON.parse(jsonReescapeForParse(text));
            if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
                throw new Error("JSON must parse to an object");
            }
            return parsed as Record<string, unknown>;
        },
        [],
    );

    // Live syntax validation
    useEffect(() => {
        try {
            parsePatchText(patchText, patchFormat);
            // eslint-disable-next-line react-hooks/set-state-in-effect
            setSyntaxError(null);
        } catch (e) {
            setSyntaxError(String((e as Error).message));
        }
    }, [patchText, patchFormat, parsePatchText]);

    const switchFormat = (newFmt: PatchFormat) => {
        if (newFmt === patchFormat) return;
        try {
            const obj = parsePatchText(patchText, patchFormat);
            const converted =
                newFmt === "json"
                    ? jsonUnescapeForEdit(JSON.stringify(obj, null, 2))
                    : tomlStringifyMultiline(obj as Record<string, unknown>);
            setPatchText(converted);
            setPatchFormat(newFmt);
        } catch {
            // Cannot convert with syntax errors — switch format but keep text
            setPatchFormat(newFmt);
        }
    };

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        try {
            const patch = parsePatchText(patchText, patchFormat);
            setSyntaxError(null);
            onSubmit({
                id: tag?.id,
                display_name: displayName || undefined,
                priority,
                patch,
            });
        } catch (err) {
            setSyntaxError(String((err as Error).message));
        }
    };

    return (
        <form
            onSubmit={handleSubmit}
            className="space-y-4 rounded-lg border border-gray-200 bg-gray-50 p-4"
        >
            <h2 className="text-lg font-semibold text-gray-900">
                {tag ? "Edit Tag" : "Create Tag"}
            </h2>
            <div className="grid grid-cols-2 gap-4">
                {tag && (
                    <div>
                        <label className="block text-sm font-medium text-gray-700">
                            Tag ID
                        </label>
                        <input
                            type="text"
                            value={tag.id}
                            disabled
                            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm disabled:bg-gray-100"
                        />
                    </div>
                )}
                <div>
                    <label className="block text-sm font-medium text-gray-700">
                        Display Name
                    </label>
                    <input
                        type="text"
                        value={displayName}
                        onChange={(e) => setDisplayName(e.target.value)}
                        required={!tag}
                        placeholder="e.g. highway-sensors"
                        className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                    />
                </div>
                <div>
                    <label className="block text-sm font-medium text-gray-700">
                        Priority
                    </label>
                    <input
                        type="number"
                        value={priority}
                        onChange={(e) => setPriority(Number(e.target.value))}
                        required
                        className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                    />
                </div>
            </div>
            <div>
                <div className="flex items-center justify-between">
                    <label className="block text-sm font-medium text-gray-700">
                        Patch (EdgeSpecPatch)
                    </label>
                    <div className="flex items-center gap-2">
                        <div className="inline-flex rounded-md border border-gray-300 text-xs">
                            {(["json", "toml"] as const).map((fmt) => (
                                <button
                                    key={fmt}
                                    type="button"
                                    onClick={() => switchFormat(fmt)}
                                    className={`px-2.5 py-1 font-medium transition-colors ${patchFormat === fmt
                                        ? "bg-indigo-600 text-white"
                                        : "text-gray-600 hover:bg-gray-100"
                                        } ${fmt === "json" ? "rounded-l-md" : "rounded-r-md"}`}
                                >
                                    {fmt.toUpperCase()}
                                </button>
                            ))}
                        </div>
                        <button
                            type="button"
                            onClick={handleCopy}
                            className="rounded-md border border-gray-300 px-2.5 py-1 text-xs font-medium text-gray-600 transition-colors hover:bg-gray-100"
                        >
                            {copied ? "Copied!" : "Copy"}
                        </button>
                    </div>
                </div>
                <textarea
                    value={patchText}
                    onChange={(e) => setPatchText(e.target.value)}
                    rows={8}
                    className={`mt-1 block w-full rounded-md border px-3 py-2 font-mono text-sm shadow-sm focus:outline-none focus:ring-1 ${syntaxError
                        ? "border-red-300 focus:border-red-500 focus:ring-red-500"
                        : "border-gray-300 focus:border-indigo-500 focus:ring-indigo-500"
                        }`}
                />
                {syntaxError && (
                    <p className="mt-1 text-sm text-red-600">{syntaxError}</p>
                )}
            </div>
            <div className="flex gap-2">
                <button
                    type="submit"
                    disabled={isPending}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
                >
                    {isPending ? "Saving..." : tag ? "Update" : "Create"}
                </button>
                <button
                    type="button"
                    onClick={onCancel}
                    className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
                >
                    Cancel
                </button>
            </div>
        </form>
    );
}

function TagDetailPanel({
    tag,
    onClose,
    onEdit,
}: {
    tag: StateTag;
    onClose: () => void;
    onEdit: () => void;
}) {
    const [patchFormat, setPatchFormat] = useState<PatchFormat>("json");
    const [copied, setCopied] = useState(false);
    const [preHeight, setPreHeight] = useState(256);
    const dragStartY = useRef(0);
    const dragStartHeight = useRef(256);

    const handleResizeMouseDown = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        dragStartY.current = e.clientY;
        dragStartHeight.current = preHeight;

        const onMouseMove = (ev: MouseEvent) => {
            const delta = ev.clientY - dragStartY.current;
            setPreHeight(Math.max(80, dragStartHeight.current + delta));
        };
        const onMouseUp = () => {
            document.removeEventListener("mousemove", onMouseMove);
            document.removeEventListener("mouseup", onMouseUp);
        };
        document.addEventListener("mousemove", onMouseMove);
        document.addEventListener("mouseup", onMouseUp);
    }, [preHeight]);

    const patchText = (() => {
        try {
            if (patchFormat === "toml") {
                return tomlStringify(tag.patch as Record<string, unknown>);
            }
            return JSON.stringify(tag.patch, null, 2);
        } catch {
            return JSON.stringify(tag.patch, null, 2);
        }
    })();

    const handleCopy = useCallback(() => {
        navigator.clipboard.writeText(patchText).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        }).catch(() => { /* clipboard permission denied */ });
    }, [patchText]);

    const tc = getTagColor(tag.id);

    return (
        <div className="space-y-4 rounded-lg border border-gray-200 bg-gray-50 p-4">
            <div className="flex items-center justify-between">
                <h2 className="text-lg font-semibold text-gray-900">Tag Detail</h2>
                <span
                    className="inline-block rounded px-2 py-0.5 text-sm font-medium"
                    style={{ backgroundColor: tc.bg, color: tc.text }}
                >
                    {tag.display_name ?? "—"}
                </span>
            </div>
            <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
                <dt className="font-medium text-gray-500">ID</dt>
                <dd><TagId id={tag.id} /></dd>
                <dt className="font-medium text-gray-500">Priority</dt>
                <dd className="text-gray-900">{tag.priority}</dd>
                {tag.created_at != null && (
                    <>
                        <dt className="font-medium text-gray-500">Created</dt>
                        <dd className="text-gray-900">
                            {formatTimestamp(new Date(tag.created_at).getTime())}
                        </dd>
                    </>
                )}
                {tag.updated_at != null && (
                    <>
                        <dt className="font-medium text-gray-500">Updated</dt>
                        <dd className="text-gray-900">
                            {formatTimestamp(new Date(tag.updated_at).getTime())}
                        </dd>
                    </>
                )}
            </dl>
            <div>
                <div className="flex items-center justify-between">
                    <span className="text-sm font-medium text-gray-700">
                        Patch (EdgeSpecPatch)
                    </span>
                    <div className="flex items-center gap-2">
                        <div className="inline-flex rounded-md border border-gray-300 text-xs">
                            {(["json", "toml"] as const).map((fmt) => (
                                <button
                                    key={fmt}
                                    type="button"
                                    onClick={() => setPatchFormat(fmt)}
                                    className={`px-2.5 py-1 font-medium transition-colors ${patchFormat === fmt
                                        ? "bg-indigo-600 text-white"
                                        : "text-gray-600 hover:bg-gray-100"
                                        } ${fmt === "json" ? "rounded-l-md" : "rounded-r-md"}`}
                                >
                                    {fmt.toUpperCase()}
                                </button>
                            ))}
                        </div>
                        <button
                            type="button"
                            onClick={handleCopy}
                            className="rounded-md border border-gray-300 px-2.5 py-1 text-xs font-medium text-gray-600 transition-colors hover:bg-gray-100"
                        >
                            {copied ? "Copied!" : "Copy"}
                        </button>
                    </div>
                </div>
                <div className="relative mt-1">
                    <pre
                        className="overflow-auto rounded-md border border-gray-300 bg-white p-3 font-mono text-xs"
                        style={{ height: `${preHeight}px` }}
                    >
                        {unescapeNewlines(patchText)}
                    </pre>
                    <div
                        onMouseDown={handleResizeMouseDown}
                        className="absolute bottom-0 right-0 flex h-5 w-5 cursor-s-resize items-center justify-center rounded-br-md bg-gray-200 hover:bg-gray-300 active:bg-gray-400"
                        title="Drag to resize"
                    >
                        <GripHorizontal className="h-3 w-3 text-gray-500" />
                    </div>
                </div>
            </div>
            <div className="flex gap-2">
                <button
                    type="button"
                    onClick={onEdit}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
                >
                    Edit
                </button>
                <button
                    type="button"
                    onClick={onClose}
                    className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
                >
                    Close
                </button>
            </div>
        </div>
    );
}
