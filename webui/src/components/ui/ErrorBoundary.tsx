// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { Component, type ErrorInfo, type ReactNode } from 'react'

interface Props {
    children: ReactNode
    fallback?: ReactNode
}

interface State {
    hasError: boolean
    error: Error | null
}

/**
 * Catches unhandled errors in the React component tree and displays a
 * recoverable fallback UI instead of crashing the entire application.
 */
export class ErrorBoundary extends Component<Props, State> {
    constructor(props: Props) {
        super(props)
        this.state = { hasError: false, error: null }
    }

    static getDerivedStateFromError(error: Error): State {
        return { hasError: true, error }
    }

    componentDidCatch(error: Error, info: ErrorInfo): void {
        console.error('ErrorBoundary caught an error:', error, info.componentStack)
    }

    render(): ReactNode {
        if (this.state.hasError) {
            if (this.props.fallback) {
                return this.props.fallback
            }
            return (
                <div className="flex min-h-screen items-center justify-center bg-gray-50 p-8">
                    <div className="max-w-md rounded-lg border border-red-200 bg-white p-6 shadow-sm">
                        <h2 className="mb-2 text-lg font-semibold text-red-700">
                            Something went wrong
                        </h2>
                        <p className="mb-4 text-sm text-gray-600">
                            {this.state.error?.message ?? 'An unexpected error occurred.'}
                        </p>
                        <button
                            type="button"
                            className="rounded bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 focus:ring-offset-2"
                            onClick={() => this.setState({ hasError: false, error: null })}
                        >
                            Try again
                        </button>
                    </div>
                </div>
            )
        }
        return this.props.children
    }
}
