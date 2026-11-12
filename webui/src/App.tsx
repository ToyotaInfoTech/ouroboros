// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { RouterProvider } from '@tanstack/react-router'
import { router } from './router'
import { useWebSocket } from './hooks/useWebSocket'
import { ErrorBoundary } from './components/ui/ErrorBoundary'
import { ToastContainer } from './components/ui/ToastContainer'

export function App() {
    useWebSocket()
    return (
        <ErrorBoundary>
            <RouterProvider router={router} />
            <ToastContainer />
        </ErrorBoundary>
    )
}
