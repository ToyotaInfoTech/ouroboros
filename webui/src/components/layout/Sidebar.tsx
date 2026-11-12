// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { Link, useLocation } from "@tanstack/react-router";
import { cn } from "../../lib/utils";
import {
    LayoutDashboard,
    Tags,
    Monitor,
    Database,
    BarChart3,
    Cpu,
    ExternalLink,
    type LucideIcon,
} from "lucide-react";

interface NavItem {
    label: string;
    href: string;
    icon: LucideIcon;
    external?: boolean;
    externalHref?: string;
}

const navItems: NavItem[] = [
    { label: "Dashboard", href: "/", icon: LayoutDashboard },
    { label: "Tags", href: "/tags", icon: Tags },
    { label: "Edges", href: "/edges", icon: Monitor },
    { label: "Metadata", href: "/metadata", icon: Database },
    { label: "Grafana", icon: BarChart3, href: "#", external: true, externalHref: "http://localhost:3000/" },
    { label: "Ray", icon: Cpu, href: "#", external: true, externalHref: "http://localhost:8265/" },
];

export function Sidebar() {
    const location = useLocation();

    return (
        <aside className="flex h-screen w-56 flex-col border-r border-gray-200 bg-gray-50">
            <div className="flex h-14 items-center gap-2 border-b border-gray-200 px-4">
                <div className="h-7 w-7 rounded-md bg-indigo-600 text-center text-sm font-bold leading-7 text-white">
                    O
                </div>
                <span className="text-sm font-semibold text-gray-900">Ouroboros</span>
            </div>
            <nav className="flex-1 space-y-1 px-2 py-3">
                {navItems.map((item) => {
                    if (item.external) {
                        return (
                            <a
                                key={item.label}
                                href={item.externalHref}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium text-gray-600 transition-colors hover:bg-gray-100 hover:text-gray-900"
                            >
                                <item.icon className="h-4 w-4" />
                                {item.label}
                                <ExternalLink className="ml-auto h-3.5 w-3.5 text-gray-400" />
                            </a>
                        );
                    }
                    const active =
                        item.href === "/"
                            ? location.pathname === "/"
                            : location.pathname.startsWith(item.href);
                    return (
                        <Link
                            key={item.href}
                            to={item.href}
                            className={cn(
                                "flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                                active
                                    ? "bg-indigo-50 text-indigo-700"
                                    : "text-gray-600 hover:bg-gray-100 hover:text-gray-900",
                            )}
                        >
                            <item.icon className="h-4 w-4" />
                            {item.label}
                        </Link>
                    );
                })}
            </nav>
        </aside>
    );
}
