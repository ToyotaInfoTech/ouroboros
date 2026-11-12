// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { create } from "zustand";

export interface GeoBounds {
    min_lat: number;
    max_lat: number;
    min_lng: number;
    max_lng: number;
}

interface MetadataFilterState {
    vinPattern: string;
    modelFilter: string;
    timeStart: string;
    timeEnd: string;
    geoBounds: GeoBounds | null;

    setVinPattern: (v: string) => void;
    setModelFilter: (v: string) => void;
    setTimeStart: (v: string) => void;
    setTimeEnd: (v: string) => void;
    setGeoBounds: (b: GeoBounds | null) => void;
}

export const useMetadataFilterStore = create<MetadataFilterState>((set) => ({
    vinPattern: "",
    modelFilter: "",
    timeStart: "",
    timeEnd: "",
    geoBounds: null,

    setVinPattern: (vinPattern) => set({ vinPattern }),
    setModelFilter: (modelFilter) => set({ modelFilter }),
    setTimeStart: (timeStart) => set({ timeStart }),
    setTimeEnd: (timeEnd) => set({ timeEnd }),
    setGeoBounds: (geoBounds) => set({ geoBounds }),
}));
