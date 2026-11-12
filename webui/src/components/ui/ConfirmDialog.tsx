// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import type { ReactNode } from "react";

interface ConfirmDialogProps {
    title?: string;
    children: ReactNode;
    onConfirm: () => void;
    onCancel: () => void;
}

export function ConfirmDialog({
    title = "Confirm Action",
    children,
    onConfirm,
    onCancel,
}: ConfirmDialogProps) {
    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30">
            <div className="w-full max-w-sm rounded-lg bg-white p-6 shadow-xl">
                <h2 className="text-lg font-semibold text-gray-900">{title}</h2>
                <div className="mt-2 text-sm text-gray-600">{children}</div>
                <div className="mt-4 flex justify-end gap-2">
                    <button
                        onClick={onCancel}
                        className="rounded border border-gray-300 px-4 py-1.5 text-sm hover:bg-gray-50"
                    >
                        No
                    </button>
                    <button
                        onClick={onConfirm}
                        className="rounded bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-700"
                    >
                        Yes
                    </button>
                </div>
            </div>
        </div>
    );
}
