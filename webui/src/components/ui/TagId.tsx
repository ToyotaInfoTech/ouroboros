// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useCallback, useState } from "react";
import { Copy, Check } from "lucide-react";

interface TagIdProps {
    id: string;
}

function shorten(id: string): string {
    if (id.length <= 12) return id;
    return `${id.slice(0, 4)}..${id.slice(-4)}`;
}

export function TagId({ id }: TagIdProps) {
    const [copied, setCopied] = useState(false);

    const handleCopy = useCallback(() => {
        navigator.clipboard.writeText(id).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
        }).catch(() => { /* clipboard permission denied */ });
    }, [id]);

    return (
        <span className="inline-flex items-center gap-1 font-mono text-sm">
            <span title={id}>{shorten(id)}</span>
            <button
                onClick={handleCopy}
                className="inline-flex items-center rounded p-0.5 text-gray-400 hover:text-gray-600"
                title="Copy ID"
            >
                {copied ? (
                    <Check className="h-3.5 w-3.5 text-green-500" />
                ) : (
                    <Copy className="h-3.5 w-3.5" />
                )}
            </button>
        </span>
    );
}
