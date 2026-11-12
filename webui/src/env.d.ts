// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

/** Build-time settings read through `import.meta.env`. */
interface ImportMetaEnv {
    /** Raster tile URL template, e.g. `https://tiles.example.com/{z}/{x}/{y}.png`. */
    readonly VITE_MAP_TILE_URL?: string;
    /** Attribution text required by the tile provider. */
    readonly VITE_MAP_ATTRIBUTION?: string;
}

interface ImportMeta {
    readonly env: ImportMetaEnv;
}
