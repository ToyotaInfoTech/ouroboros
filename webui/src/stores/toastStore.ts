// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { create } from 'zustand'

export type ToastLevel = 'info' | 'success' | 'warning' | 'error'

interface Toast {
    id: number
    message: string
    level: ToastLevel
}

interface ToastStore {
    toasts: Toast[]
    addToast: (message: string, level?: ToastLevel) => void
    removeToast: (id: number) => void
}

let nextId = 0

export const useToastStore = create<ToastStore>((set) => ({
    toasts: [],
    addToast: (message, level = 'info') => {
        const id = nextId++
        set((state) => ({ toasts: [...state.toasts, { id, message, level }] }))
        // Auto-dismiss after 5 seconds
        setTimeout(() => {
            set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }))
        }, 5000)
    },
    removeToast: (id) =>
        set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })),
}))

/** Convenience shorthand used in mutation callbacks. */
export const toast = {
    info: (msg: string) => useToastStore.getState().addToast(msg, 'info'),
    success: (msg: string) => useToastStore.getState().addToast(msg, 'success'),
    warning: (msg: string) => useToastStore.getState().addToast(msg, 'warning'),
    error: (msg: string) => useToastStore.getState().addToast(msg, 'error'),
}
