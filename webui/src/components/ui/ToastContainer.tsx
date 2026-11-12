// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useToastStore, type ToastLevel } from '../../stores/toastStore'

const levelStyles: Record<ToastLevel, string> = {
    info: 'bg-blue-50 border-blue-200 text-blue-800',
    success: 'bg-green-50 border-green-200 text-green-800',
    warning: 'bg-yellow-50 border-yellow-200 text-yellow-800',
    error: 'bg-red-50 border-red-200 text-red-800',
}

/**
 * Renders stacked toast notifications at the top-right corner of the viewport.
 * Should be placed once near the root of the component tree.
 */
export function ToastContainer() {
    const toasts = useToastStore((s) => s.toasts)
    const removeToast = useToastStore((s) => s.removeToast)

    if (toasts.length === 0) return null

    return (
        <div className="pointer-events-none fixed right-4 top-4 z-50 flex flex-col gap-2">
            {toasts.map((t) => (
                <div
                    key={t.id}
                    className={`pointer-events-auto flex max-w-sm items-start gap-2 rounded-lg border p-3 shadow-md ${levelStyles[t.level]}`}
                >
                    <p className="flex-1 text-sm whitespace-pre-wrap">{t.message}</p>
                    <button
                        type="button"
                        className="ml-2 text-current opacity-50 hover:opacity-100"
                        onClick={() => removeToast(t.id)}
                        aria-label="Dismiss notification"
                    >
                        ✕
                    </button>
                </div>
            ))}
        </div>
    )
}
